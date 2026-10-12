//! Watched-file work converges without a rescue event.
//!
//! The index writer applies events one at a time in arrival order, so an
//! event that arrives while earlier work is in flight is applied after it,
//! and the closed files it touches converge to their current bytes with
//! no later event. Shutdown ends that work: nothing is published after
//! the shutdown response.
//!
//! `a.R`/`b.R`/`c.R` carry directly checkable errors (`never_bound_*`,
//! RY010) and every final state is checked against `ry check`.

mod harness;

use harness::{
    ClientSession, Published, file_uri, join_session, normalize_diagnostics,
    published_from_cli_value, spawn_watching_session, sync_barrier,
};
use ry_lsp::test_seam::Seam;
use ry_testkit::{CliProcess, FixtureProject, rpc_receive_timeout};
use serde_json::{Value, json};

/// Await the next publication for `uri` whose diagnostics satisfy
/// `matches`, skipping intermediate publications from earlier passes.
/// Bounded by `tries` awaited publications.
async fn await_diagnostics_where(
    session: &mut ClientSession,
    uri: &str,
    matches: impl Fn(&[Value]) -> bool,
    tries: usize,
) -> Vec<Value> {
    let mut mark = session.publication_mark();
    for attempt in 0..tries {
        let publish = session
            .published_diagnostics_after(uri, mark)
            .await
            .unwrap_or_else(|error| {
                panic!("publication await failed on attempt {attempt}: {error}")
            });
        let diagnostics = normalize_diagnostics(&publish);
        if matches(&diagnostics) {
            return diagnostics;
        }
        mark = session.publication_mark();
    }
    panic!("no publication for {uri} matched within {tries} attempts");
}

fn has_ry010(diagnostics: &[Value]) -> bool {
    diagnostics.iter().any(|d| d["code"] == json!("RY010"))
}

/// `ry check --output-format json` on the fixture tree: the fresh-analysis
/// oracle the convergence assertions are measured against.
fn cli_diagnostics(fixture: &FixtureProject) -> Vec<Published> {
    let output = CliProcess::new(harness::ry_binary())
        .check(fixture, fixture.root(), ["--output-format", "json"])
        .unwrap();
    assert!(
        matches!(output.status.code(), Some(0 | 1)),
        "CLI failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let values: Vec<Value> = serde_json::from_slice(&output.stdout).unwrap();
    values
        .into_iter()
        .map(|value| published_from_cli_value(&value, fixture.root()))
        .collect()
}

fn cli_flags(fixture: &FixtureProject, name: &str) -> bool {
    cli_diagnostics(fixture)
        .iter()
        .any(|p| p.path.ends_with(name) && p.code == "RY010")
}

struct Settled {
    fixture: FixtureProject,
    session: ClientSession,
    server: tokio::task::JoinHandle<()>,
    a_uri: String,
    b_uri: String,
    c_uri: String,
}

impl Settled {
    fn uri(&self, name: &str) -> String {
        file_uri(&self.fixture.path(name))
    }

    async fn watched(&mut self, uri: &str) {
        self.session
            .notify(
                "workspace/didChangeWatchedFiles",
                json!({"changes": [{"uri": uri, "type": 2}]}),
            )
            .await
            .unwrap();
    }
}

/// A watching session over `main.R` (clean), `a.R` (`a_initial`), and
/// clean `b.R`/`c.R`. Opening `main.R` proves the initial index installed
/// (its pass also publishes `a.R`), and closing it again leaves no
/// document open; the close's clear is the last thing the writer does
/// for it, so the writer is idle on return.
async fn settled(a_initial: &str) -> Settled {
    let fixture = FixtureProject::empty().unwrap();
    fixture.write_file("main.R", "w <- 1L\n").unwrap();
    fixture.write_file("a.R", a_initial).unwrap();
    fixture.write_file("b.R", "o <- 1L\n").unwrap();
    fixture.write_file("c.R", "p <- 1L\n").unwrap();
    let main_uri = file_uri(&fixture.path("main.R"));
    let a_uri = file_uri(&fixture.path("a.R"));
    let b_uri = file_uri(&fixture.path("b.R"));
    let c_uri = file_uri(&fixture.path("c.R"));
    let (mut session, server) = spawn_watching_session(&[fixture.root()]).await;
    sync_barrier(&mut session, &main_uri).await;

    let mark = session.publication_mark();
    session.open(&main_uri, 1, "w <- 1L\n").await.unwrap();
    session
        .published_diagnostics_after(&main_uri, mark)
        .await
        .unwrap();
    session
        .published_diagnostics_after(&a_uri, mark)
        .await
        .unwrap();

    let close_mark = session.publication_mark();
    session
        .notify(
            "textDocument/didClose",
            json!({"textDocument": {"uri": main_uri}}),
        )
        .await
        .unwrap();
    let cleared = session
        .published_diagnostics_after(&main_uri, close_mark)
        .await
        .unwrap();
    assert!(normalize_diagnostics(&cleared).is_empty());
    sync_barrier(&mut session, &main_uri).await;

    Settled {
        fixture,
        session,
        server,
        a_uri,
        b_uri,
        c_uri,
    }
}

/// Send a watched event for a nonexistent `DESCRIPTION`, which the server
/// classifies as a resolution change: a full scan the test can park.
async fn trigger_full_scan(settled: &mut Settled) {
    let fake = format!("{}/DESCRIPTION", file_uri(settled.fixture.root()));
    settled.watched(&fake).await;
}

/// Events that arrive while a full scan is in flight queue behind it and
/// still converge: `a.R` is fixed and `b.R` broken on disk while the scan
/// that read the old bytes waits to install, then silence.
#[tokio::test]
async fn events_queued_behind_a_scan_converge() {
    let mut settled = settled("x <- never_bound_here\n").await;
    assert!(cli_flags(&settled.fixture, "a.R"));

    ry_lsp::test_seam::arm(Seam::Scan);
    trigger_full_scan(&mut settled).await;
    tokio::time::timeout(
        rpc_receive_timeout(),
        ry_lsp::test_seam::wait_arrived(Seam::Scan),
    )
    .await
    .expect("the scan must park before its install");

    settled.fixture.write_file("a.R", "x <- 1L\n").unwrap();
    let a_uri = settled.a_uri.clone();
    settled.watched(&a_uri).await;
    settled
        .fixture
        .write_file("b.R", "o <- never_bound_midflight\n")
        .unwrap();
    let b_uri = settled.b_uri.clone();
    settled.watched(&b_uri).await;
    ry_lsp::test_seam::release(Seam::Scan);

    let a = await_diagnostics_where(&mut settled.session, &a_uri, |d| !has_ry010(d), 4).await;
    assert!(
        !has_ry010(&a),
        "a.R must converge to its fixed bytes: {a:?}"
    );
    let b = await_diagnostics_where(&mut settled.session, &b_uri, has_ry010, 4).await;
    assert!(has_ry010(&b), "b.R's event must not be lost: {b:?}");
    assert!(!cli_flags(&settled.fixture, "a.R") && cli_flags(&settled.fixture, "b.R"));

    join_session(settled.session, settled.server).await;
}

/// A burst of events over two paths, delivered back to back, converges
/// to the final bytes of both.
#[tokio::test]
async fn event_burst_converges() {
    let mut settled = settled("x <- 1L\n").await;
    let (b_uri, c_uri) = (settled.b_uri.clone(), settled.c_uri.clone());
    for index in 0..6 {
        settled
            .fixture
            .write_file("b.R", format!("o <- {index}L\n"))
            .unwrap();
        settled.watched(&b_uri).await;
        settled
            .fixture
            .write_file("c.R", format!("p <- never_bound_{index}\n"))
            .unwrap();
        settled.watched(&c_uri).await;
    }
    settled
        .fixture
        .write_file("b.R", "o <- never_bound_final\n")
        .unwrap();
    settled.watched(&b_uri).await;
    settled.fixture.write_file("c.R", "p <- 1L\n").unwrap();
    settled.watched(&c_uri).await;

    let b = await_diagnostics_where(&mut settled.session, &b_uri, has_ry010, 6).await;
    assert!(
        has_ry010(&b),
        "the burst's final broken b.R must reach the client"
    );
    let c = await_diagnostics_where(&mut settled.session, &c_uri, |d| !has_ry010(d), 6).await;
    assert!(
        !has_ry010(&c),
        "the burst's final clean c.R must reach the client"
    );
    assert!(cli_flags(&settled.fixture, "b.R") && !cli_flags(&settled.fixture, "c.R"));

    join_session(settled.session, settled.server).await;
}

/// Shutdown requested while the writer is busy with a scan and a watched
/// refresh is queued behind it: the response arrives once the writer
/// stopped, and no publication follows it.
#[tokio::test]
async fn shutdown_with_queued_work_publishes_nothing_afterwards() {
    let mut settled = settled("x <- never_bound_here\n").await;

    ry_lsp::test_seam::arm(Seam::Scan);
    trigger_full_scan(&mut settled).await;
    tokio::time::timeout(
        rpc_receive_timeout(),
        ry_lsp::test_seam::wait_arrived(Seam::Scan),
    )
    .await
    .expect("the scan must park before its install");
    settled.fixture.write_file("a.R", "x <- 1L\n").unwrap();
    let a_uri = settled.a_uri.clone();
    settled.watched(&a_uri).await;

    // The raw request/response pair, not `LspSession::shutdown`, which
    // also sends `exit` and would end the server regardless.
    let (response, ()) = tokio::join!(settled.session.request_without_params("shutdown"), async {
        ry_lsp::test_seam::release(Seam::Scan)
    },);
    response.unwrap();
    let mark = settled.session.publication_mark();
    let leaked = tokio::time::timeout(
        std::time::Duration::from_millis(700),
        settled.session.published_diagnostics_after(&a_uri, mark),
    )
    .await;
    assert!(
        leaked.is_err(),
        "no publication may follow the shutdown response; got {:?}",
        leaked.ok()
    );

    settled.session.notify("exit", Value::Null).await.unwrap();
    drop(settled.session);
    let _ = tokio::time::timeout(std::time::Duration::from_secs(3), settled.server).await;
}

/// Ending the session right after a burst, with work still queued,
/// terminates cleanly.
#[tokio::test]
async fn teardown_with_queued_work_terminates() {
    let mut settled = settled("x <- never_bound_here\n").await;
    settled.fixture.write_file("a.R", "x <- 1L\n").unwrap();
    for name in ["a.R", "b.R", "c.R", "main.R"] {
        let uri = settled.uri(name);
        settled.watched(&uri).await;
    }
    join_session(settled.session, settled.server).await;
}
