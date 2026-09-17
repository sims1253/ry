//! #487: nested R packages under one workspace folder must not share one `Project`.
//!
//! The CLI groups files by nearest-`DESCRIPTION` ancestor
//! (`ry_workspace::group_by_package_root`): each package gets its own
//! checker `Project`, so a top-level binding in one package never resolves
//! in another. The LSP used to check a whole workspace folder through one
//! folder-wide `ProjectCache` (and one folder-wide `loaded` union), leaking
//! package-private bindings across packages: real RY010 findings were
//! suppressed, and same-named functions could resolve to the wrong
//! package's definition. These tests pin the CLI's isolation contract on
//! the editor side: sibling packages, nested roots, and duplicate function
//! names stay isolated, while files inside one package — and plain
//! non-package scripts — keep sharing definitions.

mod harness;

use harness::{
    Published, file_uri, join_session, published_from_cli_value, published_from_lsp, ry_binary,
    spawn_session, sync_barrier,
};
use ry_testkit::{CliProcess, FixtureProject};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::time::Duration;

fn run<F>(future: F)
where
    F: std::future::Future<Output = ()>,
{
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(future);
}

/// The diagnostic codes published for one URI, in publication order.
fn codes(publish: &[Value]) -> Vec<&str> {
    publish
        .iter()
        .filter_map(|diagnostic| diagnostic.get("code")?.as_str())
        .collect()
}

fn mentions(publish: &[Value], code: &str, needle: &str) -> bool {
    publish.iter().any(|diagnostic| {
        diagnostic.get("code").and_then(Value::as_str) == Some(code)
            && diagnostic
                .get("message")
                .and_then(Value::as_str)
                .is_some_and(|message| message.contains(needle))
    })
}

/// Open every listed file (relative path plus buffer text) and drain the
/// resulting broadcast. The opens land in one debounce burst, so one pass
/// publishes every open URI: quiescing on the last-opened URI collects the
/// whole burst. Returns URI -> diagnostics.
async fn open_and_quiesce(
    session: &mut harness::ClientSession,
    fixture: &FixtureProject,
    files: &[(&str, &str)],
) -> std::collections::BTreeMap<String, Vec<Value>> {
    let mark = session.publication_mark();
    let mut last_uri = String::new();
    for (relative, text) in files {
        let uri = file_uri(&fixture.path(relative));
        session.open(&uri, 1, text).await.unwrap();
        last_uri = uri;
    }
    session
        .quiesce_diagnostics(&last_uri, mark, Duration::from_millis(500))
        .await
        .unwrap()
}

const DESC_A: &str = "Package: pkgA\nVersion: 0.0.1\n";
const DESC_B: &str = "Package: pkgB\nVersion: 0.0.1\n";

/// Two sibling packages in one folder: a private binding in A must stay
/// unresolved in B. Before the fix the folder-wide project resolved it
/// from A and the RY010 never fired.
#[test]
fn sibling_packages_do_not_share_top_level_bindings() {
    run(async {
        let fixture = FixtureProject::empty().unwrap();
        fixture.write_file("pkgA/DESCRIPTION", DESC_A).unwrap();
        fixture
            .write_file("pkgA/R/a.R", "ry_review_private_a <- 1L\n")
            .unwrap();
        fixture.write_file("pkgB/DESCRIPTION", DESC_B).unwrap();
        fixture
            .write_file("pkgB/R/b.R", "x <- ry_review_private_a\n")
            .unwrap();
        let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;
        sync_barrier(&mut session, &file_uri(&fixture.path("pkgB/R/b.R"))).await;

        let burst = open_and_quiesce(
            &mut session,
            &fixture,
            &[
                ("pkgA/R/a.R", "ry_review_private_a <- 1L\n"),
                ("pkgB/R/b.R", "x <- ry_review_private_a\n"),
            ],
        )
        .await;
        let a_uri = file_uri(&fixture.path("pkgA/R/a.R"));
        let b_uri = file_uri(&fixture.path("pkgB/R/b.R"));
        let a_diags = burst.get(&a_uri).cloned().unwrap_or_default();
        let b_diags = burst.get(&b_uri).cloned().unwrap_or_default();
        assert!(
            a_diags.is_empty(),
            "pkgA's own definition must stay clean, got: {a_diags:?}"
        );
        assert!(
            mentions(&b_diags, "RY010", "ry_review_private_a"),
            "pkgB must not resolve pkgA's private binding; expected RY010, got: {b_diags:?}"
        );

        join_session(session, server).await;
    });
}

/// A package nested inside another package's tree resolves against its
/// nearest `DESCRIPTION` root: the inner file must not see the outer
/// package's private binding.
#[test]
fn nested_package_roots_isolate() {
    run(async {
        let fixture = FixtureProject::empty().unwrap();
        fixture
            .write_file("DESCRIPTION", "Package: outerpkg\nVersion: 0.0.1\n")
            .unwrap();
        fixture
            .write_file("R/outer.R", "ry_review_outer_private <- 1L\n")
            .unwrap();
        fixture
            .write_file("sub/DESCRIPTION", "Package: innerpkg\nVersion: 0.0.1\n")
            .unwrap();
        fixture
            .write_file("sub/R/inner.R", "x <- ry_review_outer_private\n")
            .unwrap();
        let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;
        sync_barrier(&mut session, &file_uri(&fixture.path("sub/R/inner.R"))).await;

        let burst = open_and_quiesce(
            &mut session,
            &fixture,
            &[
                ("R/outer.R", "ry_review_outer_private <- 1L\n"),
                ("sub/R/inner.R", "x <- ry_review_outer_private\n"),
            ],
        )
        .await;
        let outer_uri = file_uri(&fixture.path("R/outer.R"));
        let inner_uri = file_uri(&fixture.path("sub/R/inner.R"));
        let outer_diags = burst.get(&outer_uri).cloned().unwrap_or_default();
        let inner_diags = burst.get(&inner_uri).cloned().unwrap_or_default();
        assert!(
            outer_diags.is_empty(),
            "the outer package's own definition must stay clean, got: {outer_diags:?}"
        );
        assert!(
            mentions(&inner_diags, "RY010", "ry_review_outer_private"),
            "the nested package must not resolve the outer package's private binding; \
             expected RY010, got: {inner_diags:?}"
        );

        join_session(session, server).await;
    });
}

/// The same function name defined in both packages resolves per package:
/// `pkgA`'s integer `ry_review_dup` keeps its caller clean while `pkgB`'s
/// character `ry_review_dup` flags its own caller with RY040. Before the
/// fix one pooled project let the later file's definition win for both
/// callers, so `pkgA`'s caller was flagged too.
#[test]
fn duplicate_function_names_resolve_within_their_package() {
    run(async {
        let fixture = FixtureProject::empty().unwrap();
        fixture.write_file("pkgA/DESCRIPTION", DESC_A).unwrap();
        fixture
            .write_file("pkgA/R/dup.R", "ry_review_dup <- function() 1L\n")
            .unwrap();
        fixture
            .write_file("pkgA/R/use.R", "x <- ry_review_dup() + 1L\n")
            .unwrap();
        fixture.write_file("pkgB/DESCRIPTION", DESC_B).unwrap();
        fixture
            .write_file("pkgB/R/dup.R", "ry_review_dup <- function() \"str\"\n")
            .unwrap();
        fixture
            .write_file("pkgB/R/use.R", "y <- ry_review_dup() + 1L\n")
            .unwrap();
        let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;
        sync_barrier(&mut session, &file_uri(&fixture.path("pkgA/R/use.R"))).await;

        let burst = open_and_quiesce(
            &mut session,
            &fixture,
            &[
                ("pkgA/R/dup.R", "ry_review_dup <- function() 1L\n"),
                ("pkgA/R/use.R", "x <- ry_review_dup() + 1L\n"),
                ("pkgB/R/dup.R", "ry_review_dup <- function() \"str\"\n"),
                ("pkgB/R/use.R", "y <- ry_review_dup() + 1L\n"),
            ],
        )
        .await;
        let use_a = burst
            .get(&file_uri(&fixture.path("pkgA/R/use.R")))
            .cloned()
            .unwrap_or_default();
        let use_b = burst
            .get(&file_uri(&fixture.path("pkgB/R/use.R")))
            .cloned()
            .unwrap_or_default();
        assert!(
            !codes(&use_a).contains(&"RY040"),
            "pkgA's caller uses pkgA's integer definition and must stay clean, got: {use_a:?}"
        );
        assert!(
            codes(&use_b).contains(&"RY040"),
            "pkgB's caller uses pkgB's character definition and must flag RY040, got: {use_b:?}"
        );

        join_session(session, server).await;
    });
}

/// Isolation must not disable legitimate cross-file checking inside one
/// package: two files in `pkgA` still share definitions (a call into the
/// sibling file's function resolves), while `pkgB` cannot see them. The
/// cross-package probe reads the helper without calling it: the checker
/// flags unresolved reads with RY010 but stays quiet on bare unknown
/// calls (verified against `ry check`), so a call-shaped probe could not
/// distinguish isolation from that leniency.
#[test]
fn same_package_files_still_share_definitions() {
    run(async {
        let fixture = FixtureProject::empty().unwrap();
        fixture.write_file("pkgA/DESCRIPTION", DESC_A).unwrap();
        fixture
            .write_file("pkgA/R/def.R", "ry_review_pkg_helper <- function() 1L\n")
            .unwrap();
        fixture
            .write_file("pkgA/R/call.R", "x <- ry_review_pkg_helper() + 1L\n")
            .unwrap();
        fixture.write_file("pkgB/DESCRIPTION", DESC_B).unwrap();
        fixture
            .write_file("pkgB/R/sneak.R", "y <- ry_review_pkg_helper\n")
            .unwrap();
        let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;
        sync_barrier(&mut session, &file_uri(&fixture.path("pkgA/R/call.R"))).await;

        let burst = open_and_quiesce(
            &mut session,
            &fixture,
            &[
                ("pkgA/R/def.R", "ry_review_pkg_helper <- function() 1L\n"),
                ("pkgA/R/call.R", "x <- ry_review_pkg_helper() + 1L\n"),
                ("pkgB/R/sneak.R", "y <- ry_review_pkg_helper\n"),
            ],
        )
        .await;
        let call = burst
            .get(&file_uri(&fixture.path("pkgA/R/call.R")))
            .cloned()
            .unwrap_or_default();
        let sneak = burst
            .get(&file_uri(&fixture.path("pkgB/R/sneak.R")))
            .cloned()
            .unwrap_or_default();
        assert!(
            call.is_empty(),
            "same-package cross-file calls must keep resolving, got: {call:?}"
        );
        assert!(
            mentions(&sneak, "RY010", "ry_review_pkg_helper"),
            "the other package must not see pkgA's helper; expected RY010, got: {sneak:?}"
        );

        join_session(session, server).await;
    });
}

/// Plain multi-file scripts with no `DESCRIPTION` anywhere keep today's
/// folder-wide visibility: isolating them would break ordinary
/// source()-style workflows.
#[test]
fn non_package_scripts_keep_cross_file_visibility() {
    run(async {
        let fixture = FixtureProject::empty().unwrap();
        fixture
            .write_file("s1.R", "ry_review_script_helper <- function() 1L\n")
            .unwrap();
        fixture
            .write_file("s2.R", "x <- ry_review_script_helper() + 1L\n")
            .unwrap();
        let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;
        sync_barrier(&mut session, &file_uri(&fixture.path("s2.R"))).await;

        let burst = open_and_quiesce(
            &mut session,
            &fixture,
            &[
                ("s1.R", "ry_review_script_helper <- function() 1L\n"),
                ("s2.R", "x <- ry_review_script_helper() + 1L\n"),
            ],
        )
        .await;
        let s2 = burst
            .get(&file_uri(&fixture.path("s2.R")))
            .cloned()
            .unwrap_or_default();
        assert!(
            s2.is_empty(),
            "non-package scripts must keep cross-file visibility, got: {s2:?}"
        );

        join_session(session, server).await;
    });
}

/// A mixed folder — one package plus loose scripts — isolates in both
/// directions: the scripts still share definitions among each other, but
/// a loose script cannot resolve a sibling package's private binding
/// (pre-fix the folder-wide project let it, while `ry check` never
/// did). Pins the script side of the isolation contract against the CLI
/// oracle, mirroring `nested_packages_match_cli_diagnostics`.
#[test]
fn mixed_folder_scripts_do_not_see_package_bindings() {
    run(async {
        let fixture = FixtureProject::empty().unwrap();
        fixture.write_file("pkgA/DESCRIPTION", DESC_A).unwrap();
        fixture
            .write_file("pkgA/R/a.R", "ry_review_private_a <- 1L\n")
            .unwrap();
        fixture
            .write_file("s1.R", "ry_review_script_shared <- 1L\n")
            .unwrap();
        fixture
            .write_file(
                "s2.R",
                "x <- ry_review_script_shared\ny <- ry_review_private_a\n",
            )
            .unwrap();

        let output = CliProcess::new(ry_binary())
            .check(&fixture, fixture.root(), ["--output-format", "json"])
            .unwrap();
        assert!(
            matches!(output.status.code(), Some(0 | 1)),
            "CLI failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let cli: BTreeSet<Published> = serde_json::from_slice::<Vec<Value>>(&output.stdout)
            .unwrap()
            .iter()
            .map(|value| published_from_cli_value(value, fixture.root()))
            .collect();
        assert!(
            cli.iter().any(|diagnostic| diagnostic.code == "RY010"
                && diagnostic.message.contains("ry_review_private_a")),
            "the CLI must flag the script-to-package read"
        );
        assert!(
            cli.iter()
                .all(|diagnostic| !diagnostic.message.contains("ry_review_script_shared")),
            "the CLI must keep the script-to-script read clean"
        );

        let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;
        sync_barrier(&mut session, &file_uri(&fixture.path("s2.R"))).await;
        let a_path = fixture.path("pkgA/R/a.R");
        let s1_path = fixture.path("s1.R");
        let s2_path = fixture.path("s2.R");
        let a_uri = file_uri(&a_path);
        let s1_uri = file_uri(&s1_path);
        let s2_uri = file_uri(&s2_path);
        let a_text = std::fs::read_to_string(&a_path).unwrap();
        let s1_text = std::fs::read_to_string(&s1_path).unwrap();
        let s2_text = std::fs::read_to_string(&s2_path).unwrap();
        let mark = session.publication_mark();
        session.open(&a_uri, 1, &a_text).await.unwrap();
        session.open(&s1_uri, 1, &s1_text).await.unwrap();
        session.open(&s2_uri, 1, &s2_text).await.unwrap();
        // One debounced pass publishes every URI; each await takes its
        // own publication from that pass.
        let publish_a = session
            .published_diagnostics_after(&a_uri, mark)
            .await
            .unwrap();
        let publish_s1 = session
            .published_diagnostics_after(&s1_uri, mark)
            .await
            .unwrap();
        let publish_s2 = session
            .published_diagnostics_after(&s2_uri, mark)
            .await
            .unwrap();
        let mut lsp = BTreeSet::new();
        lsp.extend(published_from_lsp(&publish_a, &a_path, fixture.root()));
        lsp.extend(published_from_lsp(&publish_s1, &s1_path, fixture.root()));
        lsp.extend(published_from_lsp(&publish_s2, &s2_path, fixture.root()));
        assert_eq!(
            lsp, cli,
            "the editor must publish exactly what `ry check` reports for the mixed folder"
        );

        join_session(session, server).await;
    });
}

/// CLI/LSP parity on the nested shape: the editor must publish exactly
/// what `ry check` reports for the same folder.
#[test]
fn nested_packages_match_cli_diagnostics() {
    run(async {
        let fixture = FixtureProject::empty().unwrap();
        fixture.write_file("pkgA/DESCRIPTION", DESC_A).unwrap();
        fixture
            .write_file("pkgA/R/a.R", "ry_review_private_a <- 1L\n")
            .unwrap();
        fixture.write_file("pkgB/DESCRIPTION", DESC_B).unwrap();
        fixture
            .write_file("pkgB/R/b.R", "x <- ry_review_private_a\n")
            .unwrap();

        let output = CliProcess::new(ry_binary())
            .check(&fixture, fixture.root(), ["--output-format", "json"])
            .unwrap();
        assert!(
            matches!(output.status.code(), Some(0 | 1)),
            "CLI failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let cli: BTreeSet<Published> = serde_json::from_slice::<Vec<Value>>(&output.stdout)
            .unwrap()
            .iter()
            .map(|value| published_from_cli_value(value, fixture.root()))
            .collect();
        assert!(
            cli.iter().any(|diagnostic| diagnostic.code == "RY010"),
            "the CLI must flag the cross-package read"
        );

        let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;
        sync_barrier(&mut session, &file_uri(&fixture.path("pkgB/R/b.R"))).await;
        let a_path = fixture.path("pkgA/R/a.R");
        let b_path = fixture.path("pkgB/R/b.R");
        let a_uri = file_uri(&a_path);
        let b_uri = file_uri(&b_path);
        let a_text = std::fs::read_to_string(&a_path).unwrap();
        let b_text = std::fs::read_to_string(&b_path).unwrap();
        let mark = session.publication_mark();
        session.open(&a_uri, 1, &a_text).await.unwrap();
        session.open(&b_uri, 1, &b_text).await.unwrap();
        // One debounced pass publishes both URIs; each await takes its
        // own publication from that pass.
        let publish_a = session
            .published_diagnostics_after(&a_uri, mark)
            .await
            .unwrap();
        let publish_b = session
            .published_diagnostics_after(&b_uri, mark)
            .await
            .unwrap();
        let mut lsp = BTreeSet::new();
        lsp.extend(published_from_lsp(&publish_a, &a_path, fixture.root()));
        lsp.extend(published_from_lsp(&publish_b, &b_path, fixture.root()));
        assert_eq!(
            lsp, cli,
            "the editor must publish exactly what `ry check` reports"
        );

        join_session(session, server).await;
    });
}
