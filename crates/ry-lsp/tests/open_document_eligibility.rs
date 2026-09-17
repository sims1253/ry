//! #488: open documents must honor the `index.max-file-bytes` and
//! `index.max-depth` eligibility gates, not just `exclude` patterns.
//!
//! A file the bounded background discovery omits — over the size cap, or
//! below a pruned depth — used to enter project-wide analysis as soon as
//! it was opened: `eligibility_for_path` enforced excludes and folder
//! enablement only, and the admitted buffer's definitions leaked into
//! every other open file's resolution, contradicting the documented
//! editor contract in `docs/configuration.md`.
//!
//! Eligibility now runs through the shared
//! `ry_workspace::is_file_eligible_with_limits` policy, so the opened
//! buffer clears exactly what the closed index enforces. Two nuances,
//! both pinned here: the size gate measures the open buffer's text
//! length (unsaved pasted content crosses the boundary without touching
//! disk, so `fs::metadata` must not stand in), and the depth gate counts
//! the containing directory's components relative to the folder root
//! (walk depth is not a path property).
//!
//! `use.R` calls `f()` in a shape that mismatches only for the character
//! variant; `def.R` (the oversized / deep file) defines the character
//! `f`, so the leak is observable as an RY040 in `use.R`, exactly the
//! shadowing-observable idiom `cross_file_shadowing.rs` uses.

mod harness;

use harness::{file_uri, join_session, spawn_session, sync_barrier};
use ry_testkit::FixtureProject;
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// Idle window for `quiesce_diagnostics`; comfortably exceeds the
/// server's 180ms debounce so a multi-URI burst is fully collected.
const DRAIN: std::time::Duration = std::time::Duration::from_millis(500);

fn run<F, T>(future: F) -> T
where
    F: std::future::Future<Output = T>,
{
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(future)
}

/// `def.R` defines the character `f`; `use.R` calls `f() + 1L`, which
/// mismatches (RY040) exactly when that definition participates. The
/// definition buffer is 41 bytes, comfortably over the 32-byte test cap
/// the oversized tests configure.
const DEF_CHAR: &str = "f <- function(argument) \"str_value\"\n";
const USE: &str = "x <- f() + 1L\n";

fn has_ry040(publish: &Value) -> bool {
    publish
        .pointer("/params/diagnostics")
        .and_then(Value::as_array)
        .is_some_and(|diagnostics| {
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic["code"] == json!("RY040"))
        })
}

fn last_publication<'a>(store: &'a BTreeMap<String, Vec<Value>>, uri: &str) -> &'a [Value] {
    store.get(uri).map(Vec::as_slice).unwrap_or(&[])
}

/// Closed-to-open parity for the size gate: with `max-file-bytes` below
/// the definition file's size, indexing omits it while closed and opening
/// the identical buffer must not admit it either — `use.R` stays clean.
/// This is the issue's verbatim repro.
#[test]
fn oversized_open_buffer_does_not_enter_project_state() {
    run(async {
        let fixture = FixtureProject::empty().unwrap();
        fixture
            .write_file("ry.toml", "[index]\nmax-file-bytes = 32\n")
            .unwrap();
        fixture.write_file("def.R", DEF_CHAR).unwrap();
        fixture.write_file("use.R", USE).unwrap();
        assert!(
            DEF_CHAR.len() > 32,
            "the definition buffer must exceed the cap"
        );
        let def_uri = file_uri(&fixture.path("def.R"));
        let use_uri = file_uri(&fixture.path("use.R"));
        let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;
        sync_barrier(&mut session, &use_uri).await;

        let mark = session.publication_mark();
        session.open(&use_uri, 1, USE).await.unwrap();
        session.open(&def_uri, 1, DEF_CHAR).await.unwrap();
        let publish = session
            .published_diagnostics_after(&use_uri, mark)
            .await
            .unwrap();
        assert!(
            !has_ry040(&publish),
            "the oversized buffer's character f must not leak into use.R: {publish}"
        );
        // The ineligible document itself is acknowledged with an empty
        // publication: it stays out of project-wide state without leaving
        // stale squiggles.
        let def_publish = session
            .published_diagnostics_after(&def_uri, mark)
            .await
            .unwrap();
        assert!(
            !has_ry040(&def_publish),
            "the oversized buffer must not carry project diagnostics either: {def_publish}"
        );

        join_session(session, server).await;
    })
}

/// Closed-to-open parity for the depth gate: with `max-depth = 1` the
/// walker prunes `a/b/`, and opening the identical deep buffer must not
/// admit it either.
#[test]
fn deep_open_buffer_does_not_enter_project_state() {
    run(async {
        let fixture = FixtureProject::empty().unwrap();
        fixture
            .write_file("ry.toml", "[index]\nmax-depth = 1\n")
            .unwrap();
        fixture.write_file("a/b/def.R", DEF_CHAR).unwrap();
        fixture.write_file("use.R", USE).unwrap();
        let def_uri = file_uri(&fixture.path("a/b/def.R"));
        let use_uri = file_uri(&fixture.path("use.R"));
        let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;
        sync_barrier(&mut session, &use_uri).await;

        let mark = session.publication_mark();
        session.open(&use_uri, 1, USE).await.unwrap();
        session.open(&def_uri, 1, DEF_CHAR).await.unwrap();
        let publish = session
            .published_diagnostics_after(&use_uri, mark)
            .await
            .unwrap();
        assert!(
            !has_ry040(&publish),
            "the deep buffer's character f must not leak into use.R: {publish}"
        );

        join_session(session, server).await;
    })
}

/// The depth boundary is inclusive: a file whose parent sits exactly at
/// `max-depth` is discovered closed and stays admitted open — the same
/// boundary `discover_recursive` prunes at, pinned end to end.
#[test]
fn file_at_exactly_max_depth_stays_eligible() {
    run(async {
        let fixture = FixtureProject::empty().unwrap();
        fixture
            .write_file("ry.toml", "[index]\nmax-depth = 1\n")
            .unwrap();
        fixture.write_file("a/def.R", DEF_CHAR).unwrap();
        fixture.write_file("use.R", USE).unwrap();
        let def_uri = file_uri(&fixture.path("a/def.R"));
        let use_uri = file_uri(&fixture.path("use.R"));
        let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;
        sync_barrier(&mut session, &use_uri).await;

        let mark = session.publication_mark();
        session.open(&use_uri, 1, USE).await.unwrap();
        session.open(&def_uri, 1, DEF_CHAR).await.unwrap();
        let publish = session
            .published_diagnostics_after(&use_uri, mark)
            .await
            .unwrap();
        assert!(
            has_ry040(&publish),
            "a buffer exactly at max-depth is eligible, so its character f must resolve: {publish}"
        );

        join_session(session, server).await;
    })
}

/// Edits crossing the size boundary in both directions: pasting the file
/// past the cap ejects the buffer from project state (clearing `use.R`),
/// and trimming it back re-admits it (restoring RY040). The buffer length
/// is what counts — the on-disk file is untouched throughout.
#[test]
fn edits_crossing_the_size_boundary_eject_and_readmit() {
    run(async {
        let fixture = FixtureProject::empty().unwrap();
        fixture
            .write_file("ry.toml", "[index]\nmax-file-bytes = 64\n")
            .unwrap();
        fixture.write_file("def.R", DEF_CHAR).unwrap();
        fixture.write_file("use.R", USE).unwrap();
        let def_uri = file_uri(&fixture.path("def.R"));
        let use_uri = file_uri(&fixture.path("use.R"));
        let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;
        sync_barrier(&mut session, &use_uri).await;

        // Open under the cap: the character f resolves, RY040 fires.
        let mark = session.publication_mark();
        session.open(&use_uri, 1, USE).await.unwrap();
        session.open(&def_uri, 1, DEF_CHAR).await.unwrap();
        let admitted = session
            .published_diagnostics_after(&use_uri, mark)
            .await
            .unwrap();
        assert!(
            has_ry040(&admitted),
            "the under-cap buffer must participate: {admitted}"
        );

        // Paste past the cap: the buffer leaves project state. The
        // padding is a trailing comment so the buffer stays one binding
        // and only the size changes.
        let oversized = format!("{DEF_CHAR}# {}\n", "x".repeat(64));
        assert!(oversized.len() > 64);
        let mark = session.publication_mark();
        session
            .notify(
                "textDocument/didChange",
                json!({
                    "textDocument": {"uri": def_uri, "version": 2},
                    "contentChanges": [{"text": oversized}],
                }),
            )
            .await
            .unwrap();
        let ejected = session
            .quiesce_diagnostics(&use_uri, mark, DRAIN)
            .await
            .unwrap();
        assert!(
            !last_publication(&ejected, &use_uri)
                .iter()
                .any(|diagnostic| diagnostic["code"] == json!("RY040")),
            "growing past the cap must eject the buffer; use.R must clear, got: {ejected:?}"
        );
        // The ejected document's own squiggles are reconciled away
        // through the shared #519 dropped-URI seam.
        assert!(
            last_publication(&ejected, &def_uri).is_empty(),
            "the ejected buffer must end with an empty publication, got: {ejected:?}"
        );

        // Trim back under the cap: the buffer re-enters project state.
        let mark = session.publication_mark();
        session
            .notify(
                "textDocument/didChange",
                json!({
                    "textDocument": {"uri": def_uri, "version": 3},
                    "contentChanges": [{"text": DEF_CHAR}],
                }),
            )
            .await
            .unwrap();
        let readmitted = session
            .published_diagnostics_after(&use_uri, mark)
            .await
            .unwrap();
        assert!(
            has_ry040(&readmitted),
            "shrinking back under the cap must re-admit the buffer: {readmitted}"
        );

        join_session(session, server).await;
    })
}

/// Configuration changes restoring eligibility: raising the cap
/// re-admits an ineligible open buffer without reopening it.
#[test]
fn raising_the_cap_readmits_an_ineligible_open_buffer() {
    run(async {
        let fixture = FixtureProject::empty().unwrap();
        fixture
            .write_file("ry.toml", "[index]\nmax-file-bytes = 32\n")
            .unwrap();
        fixture.write_file("def.R", DEF_CHAR).unwrap();
        fixture.write_file("use.R", USE).unwrap();
        let def_uri = file_uri(&fixture.path("def.R"));
        let use_uri = file_uri(&fixture.path("use.R"));
        let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;
        sync_barrier(&mut session, &use_uri).await;

        // Under the low cap the buffer is out: use.R is clean.
        let mark = session.publication_mark();
        session.open(&use_uri, 1, USE).await.unwrap();
        session.open(&def_uri, 1, DEF_CHAR).await.unwrap();
        let excluded = session
            .published_diagnostics_after(&use_uri, mark)
            .await
            .unwrap();
        assert!(
            !has_ry040(&excluded),
            "the over-cap buffer must stay out: {excluded}"
        );

        // Raise the cap past the buffer length: the already-open buffer
        // re-enters project state on the config reload republish.
        fixture
            .write_file("ry.toml", "[index]\nmax-file-bytes = 4096\n")
            .unwrap();
        let mark = session.publication_mark();
        session
            .notify("workspace/didChangeConfiguration", json!({"settings": {}}))
            .await
            .unwrap();
        let readmitted = session
            .published_diagnostics_after(&use_uri, mark)
            .await
            .unwrap();
        assert!(
            has_ry040(&readmitted),
            "raising the cap must re-admit the open buffer: {readmitted}"
        );

        join_session(session, server).await;
    })
}

/// Lowering the cap ejects an eligible open buffer and clears both sides:
/// `use.R` loses the cross-file RY040 and the ejected buffer's own stale
/// diagnostics are reconciled away rather than lingering.
#[test]
fn lowering_the_cap_ejects_an_open_buffer_and_clears_it() {
    run(async {
        let fixture = FixtureProject::empty().unwrap();
        fixture.write_file("def.R", DEF_CHAR).unwrap();
        fixture
            .write_file("use.R", "x <- f() + 1L\ny <- never_bound_here\n")
            .unwrap();
        let def_uri = file_uri(&fixture.path("def.R"));
        let use_uri = file_uri(&fixture.path("use.R"));
        let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;
        sync_barrier(&mut session, &use_uri).await;

        // No caps configured: the character f resolves, RY040 fires in
        // use.R alongside its own unbound-name finding.
        let mark = session.publication_mark();
        session
            .open(&use_uri, 1, "x <- f() + 1L\ny <- never_bound_here\n")
            .await
            .unwrap();
        session.open(&def_uri, 1, DEF_CHAR).await.unwrap();
        let initial = session
            .quiesce_diagnostics(&use_uri, mark, DRAIN)
            .await
            .unwrap();
        assert!(
            last_publication(&initial, &use_uri)
                .iter()
                .any(|diagnostic| diagnostic["code"] == json!("RY040")),
            "the eligible buffer must participate, got: {initial:?}"
        );

        // Lower the cap below the buffer length: the buffer leaves the
        // analysis set on the reload republish.
        fixture
            .write_file("ry.toml", "[index]\nmax-file-bytes = 32\n")
            .unwrap();
        let mark = session.publication_mark();
        session
            .notify("workspace/didChangeConfiguration", json!({"settings": {}}))
            .await
            .unwrap();
        let ejected = session
            .quiesce_diagnostics(&use_uri, mark, DRAIN)
            .await
            .unwrap();
        assert!(
            !last_publication(&ejected, &use_uri)
                .iter()
                .any(|diagnostic| diagnostic["code"] == json!("RY040")),
            "the ejected buffer's definition must stop resolving, got: {ejected:?}"
        );
        assert!(
            last_publication(&ejected, &def_uri).is_empty(),
            "the ejected buffer must end with an empty publication, got: {ejected:?}"
        );

        join_session(session, server).await;
    })
}

/// Ineligible open documents keep lightweight editor features: inlay
/// hints are computed from the single document's own parse and never
/// enter project-wide state, so they still serve a size-gated buffer
/// (while `enable: false` keeps returning null — the editor
/// intentionally disabled ry there).
#[test]
fn ineligible_open_buffer_keeps_inlay_hints() {
    run(async {
        let fixture = FixtureProject::empty().unwrap();
        fixture
            .write_file("ry.toml", "[index]\nmax-file-bytes = 32\n")
            .unwrap();
        let source = format!("my_value <- 1L\n# {}\n", "x".repeat(32));
        assert!(source.len() > 32);
        fixture.write_file("big.R", &source).unwrap();
        let uri = file_uri(&fixture.path("big.R"));
        let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;
        sync_barrier(&mut session, &uri).await;

        let mark = session.publication_mark();
        session.open(&uri, 1, &source).await.unwrap();
        let publish = session
            .published_diagnostics_after(&uri, mark)
            .await
            .unwrap();
        assert!(
            publish["params"]["diagnostics"]
                .as_array()
                .is_some_and(Vec::is_empty),
            "the over-cap buffer carries no project diagnostics: {publish}"
        );
        let hints = session
            .request(
                "textDocument/inlayHint",
                json!({
                    "textDocument": {"uri": uri},
                    "range": {"start": {"line": 0, "character": 0}, "end": {"line": 2, "character": 0}},
                }),
            )
            .await
            .unwrap();
        assert!(
            hints
                .as_array()
                .is_some_and(|hints| hints.iter().any(|hint| {
                    hint["position"] == json!({"line": 0, "character": 8})
                        && hint["label"] == json!(": integer<len=1>")
                })),
            "single-document hints must still serve the ineligible buffer: {hints}"
        );

        join_session(session, server).await;
    })
}
