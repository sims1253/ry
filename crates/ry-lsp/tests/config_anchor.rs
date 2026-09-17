//! #493: an inherited or explicit external `ry.toml` keeps its own path
//! anchor in the language server.
//!
//! The server correctly discovers a parent `ry.toml` (or loads one through
//! the `configuration` setting outside the folder), but it anchored exclude
//! matching, `include-build-ignored`, and baseline key normalization at the
//! workspace folder instead of the config's directory — while the CLI keeps
//! the discovered config directory as its config root. Config-relative
//! `exclude` patterns therefore never matched once the folder differed from
//! the config directory, and CLI-generated baseline keys (`pkgA/R/…`) never
//! matched the editor's folder-relative keys (`R/…`), so baselined findings
//! reappeared. Each folder context now records the directory of the
//! `ry.toml` it loaded and every config-relative resolution anchors there.
//! When the config lives in the folder itself the anchor equals the folder
//! root, so only inherited or external configs change behavior — which is
//! why the single-root parity fixtures never caught this.

mod harness;

use harness::{file_uri, join_session, spawn_session, sync_barrier};
use ry_testkit::{CliProcess, FixtureProject};
use serde_json::{Value, json};

/// Run a future on a current-thread tokio runtime (same pattern as the
/// session tests).
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

/// Count occurrences of a rule code in a publishDiagnostics `diagnostics`
/// array.
fn count_code(publish: &Value, code: &str) -> usize {
    publish["params"]["diagnostics"]
        .as_array()
        .expect("publishDiagnostics array")
        .iter()
        .filter(|diagnostic| diagnostic["code"] == code)
        .count()
}

/// Run the production CLI inside the fixture and return the JSON
/// diagnostic stream, asserting the run itself succeeded.
fn cli_json(fixture: &FixtureProject, target: &str, args: &[&str]) -> Vec<Value> {
    let output = CliProcess::new(harness::ry_binary())
        .check(fixture, target, args)
        .unwrap();
    assert!(
        matches!(output.status.code(), Some(0 | 1)),
        "CLI failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

/// The #493 repro with a CLI oracle: `/repo/ry.toml` carries
/// `exclude = ["pkgA/R/generated/**"]`, the editor opens `/repo/pkgA`.
/// `ry check pkgA` anchors the pattern at `/repo` and excludes the
/// generated file; the server must do the same, not anchor at the folder
/// where the pattern would have to be `R/generated/**` to match.
#[test]
fn parent_config_exclude_anchors_at_config_dir_like_cli() {
    run(async {
        let fixture = FixtureProject::empty().unwrap();
        fixture
            .write_file("ry.toml", "exclude = [\"pkgA/R/generated/**\"]\n")
            .unwrap();
        fixture
            .write_file("pkgA/R/keep.R", "x <- never_bound_here\n")
            .unwrap();
        fixture
            .write_file("pkgA/R/generated/gen.R", "x <- never_bound_here\n")
            .unwrap();

        // CLI oracle from the issue: run from the repository, target the
        // package, and the parent config's pattern fires.
        let cli = cli_json(&fixture, "pkgA", &["--output-format", "json"]);
        assert!(
            cli.iter()
                .any(|d| d["path"].as_str().unwrap().contains("keep.R")),
            "keep.R must report: {cli:?}"
        );
        assert!(
            !cli.iter()
                .any(|d| d["path"].as_str().unwrap().contains("gen.R")),
            "the CLI excludes gen.R through the parent config: {cli:?}"
        );

        let folder = fixture.path("pkgA");
        let (mut session, server) = spawn_session(&[&folder], json!({}), None).await;
        let gen_uri = file_uri(&fixture.path("pkgA/R/generated/gen.R"));
        let keep_uri = file_uri(&fixture.path("pkgA/R/keep.R"));

        let mark = session.publication_mark();
        session
            .open(&gen_uri, 1, "x <- never_bound_here\n")
            .await
            .unwrap();
        let gen_publish = session
            .published_diagnostics_after(&gen_uri, mark)
            .await
            .unwrap();
        assert_eq!(
            count_code(&gen_publish, "RY010"),
            0,
            "gen.R must be excluded through the parent config like `ry check pkgA`: {gen_publish}"
        );

        let mark = session.publication_mark();
        session
            .open(&keep_uri, 1, "x <- never_bound_here\n")
            .await
            .unwrap();
        let keep_publish = session
            .published_diagnostics_after(&keep_uri, mark)
            .await
            .unwrap();
        assert_eq!(
            count_code(&keep_publish, "RY010"),
            1,
            "keep.R is not covered by the pattern and must still report: {keep_publish}"
        );
        join_session(session, server).await;
    })
}

/// A CLI-generated baseline uses config-directory-relative keys, so a
/// `ry.toml` above the folder must anchor key normalization at its own
/// directory or every baselined finding reappears in the editor.
#[test]
fn parent_config_baseline_keys_match_in_the_editor() {
    run(async {
        let fixture = FixtureProject::empty().unwrap();
        fixture
            .write_file("ry.toml", "baseline = \"baseline.json\"\n")
            .unwrap();
        fixture
            .write_file("pkgA/R/diag.R", "x <- never_bound_here\n")
            .unwrap();

        // Generate the baseline the way a user does: `ry check pkgA
        // --write-baseline baseline.json` from the repository root. Keys
        // land relative to the config directory (`pkgA/R/diag.R`), and the
        // config rebases the relative `baseline` path onto `/repo`.
        let cli = cli_json(
            &fixture,
            "pkgA",
            &[
                "--write-baseline",
                "baseline.json",
                "--output-format",
                "json",
            ],
        );
        assert!(
            cli.iter().any(|d| d["code"] == json!("RY010")),
            "the finding to baseline: {cli:?}"
        );
        let baseline: Value =
            serde_json::from_slice(&std::fs::read(fixture.path("baseline.json")).unwrap()).unwrap();
        for entry in baseline["entries"].as_array().expect("baseline entries") {
            assert_eq!(
                entry["path"],
                json!("pkgA/R/diag.R"),
                "CLI baseline keys are config-dir relative: {baseline:?}"
            );
        }

        // A finding the baseline does not cover, proving the editor's
        // check ran rather than nothing being analyzed.
        fixture
            .write_file("pkgA/R/other.R", "z <- length(xx = 1L)\n")
            .unwrap();

        let folder = fixture.path("pkgA");
        let (mut session, server) = spawn_session(&[&folder], json!({}), None).await;
        let diag_uri = file_uri(&fixture.path("pkgA/R/diag.R"));
        let other_uri = file_uri(&fixture.path("pkgA/R/other.R"));

        let mark = session.publication_mark();
        session
            .open(&diag_uri, 1, "x <- never_bound_here\n")
            .await
            .unwrap();
        let diag_publish = session
            .published_diagnostics_after(&diag_uri, mark)
            .await
            .unwrap();
        assert_eq!(
            count_code(&diag_publish, "RY010"),
            0,
            "the baselined finding must stay baselined when the config sits above the folder: {diag_publish}"
        );

        let mark = session.publication_mark();
        session
            .open(&other_uri, 1, "z <- length(xx = 1L)\n")
            .await
            .unwrap();
        let other_publish = session
            .published_diagnostics_after(&other_uri, mark)
            .await
            .unwrap();
        assert_eq!(
            count_code(&other_publish, "RY090"),
            1,
            "an unbaselined finding in the same folder must still publish: {other_publish}"
        );
        join_session(session, server).await;
    })
}

/// Control for #493: a `ry.toml` inside the folder anchors at the folder
/// itself, exactly as before — the fix only moves the anchor for configs
/// above or outside the folder.
#[test]
fn in_folder_config_still_anchors_at_the_folder() {
    run(async {
        let fixture = FixtureProject::empty().unwrap();
        fixture
            .write_file("pkgA/ry.toml", "exclude = [\"R/generated/**\"]\n")
            .unwrap();
        fixture
            .write_file("pkgA/R/keep.R", "x <- never_bound_here\n")
            .unwrap();
        fixture
            .write_file("pkgA/R/generated/gen.R", "x <- never_bound_here\n")
            .unwrap();

        let folder = fixture.path("pkgA");
        let (mut session, server) = spawn_session(&[&folder], json!({}), None).await;
        let gen_uri = file_uri(&fixture.path("pkgA/R/generated/gen.R"));
        let keep_uri = file_uri(&fixture.path("pkgA/R/keep.R"));

        let mark = session.publication_mark();
        session
            .open(&gen_uri, 1, "x <- never_bound_here\n")
            .await
            .unwrap();
        let gen_publish = session
            .published_diagnostics_after(&gen_uri, mark)
            .await
            .unwrap();
        assert_eq!(
            count_code(&gen_publish, "RY010"),
            0,
            "folder-relative pattern against the folder root: {gen_publish}"
        );

        let mark = session.publication_mark();
        session
            .open(&keep_uri, 1, "x <- never_bound_here\n")
            .await
            .unwrap();
        let keep_publish = session
            .published_diagnostics_after(&keep_uri, mark)
            .await
            .unwrap();
        assert_eq!(
            count_code(&keep_publish, "RY010"),
            1,
            "uncovered file still reports: {keep_publish}"
        );
        join_session(session, server).await;
    })
}

/// An explicit `configuration` setting pointing outside the folder anchors
/// config-relative paths at the config's own directory. The `..` segment
/// also pins the anchor's dot-segment normalization: the raw joined parent
/// (`pkgA/..`) could never prefix-match the clean absolute paths
/// diagnostics carry.
#[test]
fn explicit_external_config_anchors_at_its_own_directory() {
    run(async {
        let fixture = FixtureProject::empty().unwrap();
        fixture
            .write_file("ry.toml", "exclude = [\"pkgA/R/generated/**\"]\n")
            .unwrap();
        fixture
            .write_file("pkgA/R/keep.R", "x <- never_bound_here\n")
            .unwrap();
        fixture
            .write_file("pkgA/R/generated/gen.R", "x <- never_bound_here\n")
            .unwrap();

        let folder = fixture.path("pkgA");
        let (mut session, server) = spawn_session(
            &[&folder],
            json!({}),
            Some(json!({
                "settings": [{"configuration": "../ry.toml"}],
                "globalSettings": {}
            })),
        )
        .await;
        let gen_uri = file_uri(&fixture.path("pkgA/R/generated/gen.R"));
        let keep_uri = file_uri(&fixture.path("pkgA/R/keep.R"));

        let mark = session.publication_mark();
        session
            .open(&gen_uri, 1, "x <- never_bound_here\n")
            .await
            .unwrap();
        let gen_publish = session
            .published_diagnostics_after(&gen_uri, mark)
            .await
            .unwrap();
        assert_eq!(
            count_code(&gen_publish, "RY010"),
            0,
            "external config's pattern must anchor at the config's directory: {gen_publish}"
        );

        let mark = session.publication_mark();
        session
            .open(&keep_uri, 1, "x <- never_bound_here\n")
            .await
            .unwrap();
        let keep_publish = session
            .published_diagnostics_after(&keep_uri, mark)
            .await
            .unwrap();
        assert_eq!(
            count_code(&keep_publish, "RY010"),
            1,
            "uncovered file still reports: {keep_publish}"
        );
        join_session(session, server).await;
    })
}

/// `include-build-ignored` patterns are `ry.toml`-relative too: a parent
/// config re-including a package's `.Rbuildignore`d `vendor/` tree must
/// make the vendor code land in the background index (the caller's
/// unresolved binding resolves), exactly as `ry check` resolves it. The
/// control session without the include keeps the binding unresolved,
/// proving the fixture's `.Rbuildignore` actually bites.
#[test]
fn parent_config_include_build_ignored_reindexes_vendor_code() {
    run(async {
        let with_include = {
            let fixture = FixtureProject::empty().unwrap();
            fixture
                .write_file("ry.toml", "include-build-ignored = [\"pkgA/vendor/**\"]\n")
                .unwrap();
            fixture
                .write_file("pkgA/DESCRIPTION", "Package: pkgA\n")
                .unwrap();
            fixture
                .write_file("pkgA/.Rbuildignore", "^vendor$\n")
                .unwrap();
            fixture
                .write_file("pkgA/vendor/consts.R", "answer <- 42L\n")
                .unwrap();
            fixture
                .write_file("pkgA/R/main.R", "x <- answer + 1L\n")
                .unwrap();
            let folder = fixture.path("pkgA");
            let (mut session, server) = spawn_session(&[&folder], json!({}), None).await;
            let main_uri = file_uri(&fixture.path("pkgA/R/main.R"));
            sync_barrier(&mut session, &main_uri).await;
            let mark = session.publication_mark();
            session
                .open(&main_uri, 1, "x <- answer + 1L\n")
                .await
                .unwrap();
            let publish = session
                .published_diagnostics_after(&main_uri, mark)
                .await
                .unwrap();
            let count = count_code(&publish, "RY010");
            join_session(session, server).await;
            count
        };
        assert_eq!(
            with_include, 0,
            "the parent config's include must land vendor/consts.R in the index so `answer` resolves"
        );

        // Control: without the include, the .Rbuildignore'd constants stay
        // out of the index and the binding is unresolved.
        let without_include = {
            let fixture = FixtureProject::empty().unwrap();
            fixture.write_file("ry.toml", "").unwrap();
            fixture
                .write_file("pkgA/DESCRIPTION", "Package: pkgA\n")
                .unwrap();
            fixture
                .write_file("pkgA/.Rbuildignore", "^vendor$\n")
                .unwrap();
            fixture
                .write_file("pkgA/vendor/consts.R", "answer <- 42L\n")
                .unwrap();
            fixture
                .write_file("pkgA/R/main.R", "x <- answer + 1L\n")
                .unwrap();
            let folder = fixture.path("pkgA");
            let (mut session, server) = spawn_session(&[&folder], json!({}), None).await;
            let main_uri = file_uri(&fixture.path("pkgA/R/main.R"));
            sync_barrier(&mut session, &main_uri).await;
            let mark = session.publication_mark();
            session
                .open(&main_uri, 1, "x <- answer + 1L\n")
                .await
                .unwrap();
            let publish = session
                .published_diagnostics_after(&main_uri, mark)
                .await
                .unwrap();
            let count = count_code(&publish, "RY010");
            join_session(session, server).await;
            count
        };
        assert_eq!(
            without_include, 1,
            "without the include the vendor constants must stay unindexed and unresolved"
        );
    })
}

/// Two sibling folders share the parent `ry.toml`; each resolves and
/// anchors it independently, so the exclude applies in the covered folder
/// without silencing the sibling.
#[test]
fn sibling_folders_each_anchor_the_shared_parent_config() {
    run(async {
        let fixture = FixtureProject::empty().unwrap();
        fixture
            .write_file("ry.toml", "exclude = [\"pkgA/R/generated/**\"]\n")
            .unwrap();
        fixture
            .write_file("pkgA/R/generated/gen.R", "x <- never_bound_here\n")
            .unwrap();
        fixture
            .write_file("pkgB/R/keep.R", "x <- never_bound_here\n")
            .unwrap();

        let pkg_a = fixture.path("pkgA");
        let pkg_b = fixture.path("pkgB");
        let (mut session, server) = spawn_session(&[&pkg_a, &pkg_b], json!({}), None).await;
        let gen_uri = file_uri(&fixture.path("pkgA/R/generated/gen.R"));
        let keep_uri = file_uri(&fixture.path("pkgB/R/keep.R"));

        let mark = session.publication_mark();
        session
            .open(&gen_uri, 1, "x <- never_bound_here\n")
            .await
            .unwrap();
        let gen_publish = session
            .published_diagnostics_after(&gen_uri, mark)
            .await
            .unwrap();
        assert_eq!(
            count_code(&gen_publish, "RY010"),
            0,
            "pkgA's excluded file stays excluded: {gen_publish}"
        );

        let mark = session.publication_mark();
        session
            .open(&keep_uri, 1, "x <- never_bound_here\n")
            .await
            .unwrap();
        let keep_publish = session
            .published_diagnostics_after(&keep_uri, mark)
            .await
            .unwrap();
        assert_eq!(
            count_code(&keep_publish, "RY010"),
            1,
            "pkgB is not covered by the pattern and must still report: {keep_publish}"
        );
        join_session(session, server).await;
    })
}

/// A retained baseline must stay paired with the anchor it was loaded
/// under. When the config moves to another directory mid-session and the
/// new baseline fails to load, retaining the old baseline would pair its
/// old-relative keys with the new anchor: a file that merely shares the
/// key shape (`R/diag.R` under the new root versus `R/diag.R` under the
/// config subdirectory) would be silently absorbed. The reload must
/// clear the baseline instead.
#[test]
fn retained_baseline_stays_paired_with_its_config_origin() {
    run(async {
        let fixture = FixtureProject::empty().unwrap();
        // The covered finding lives under the config directory so its
        // baseline key (`R/diag.R`) is relative to that directory.
        fixture
            .write_file("cfg/ry.toml", "baseline = \"b.json\"\n")
            .unwrap();
        fixture
            .write_file("cfg/R/diag.R", "x <- never_bound_here\n")
            .unwrap();
        // A same-shaped finding at the folder root, covered by no
        // baseline entry while the config lives in `cfg/`.
        fixture
            .write_file("R/diag.R", "x <- never_bound_here\n")
            .unwrap();

        // CLI-generated baseline from the config directory: exactly one
        // entry. The CLI writes input-relative keys (`cfg/R/diag.R` from
        // the fixture root); re-anchor the entry to the config directory
        // (`R/diag.R`) — the form a config-dir-relative baseline uses —
        // keeping the CLI-derived code and message untouched.
        let cli = cli_json(
            &fixture,
            "cfg",
            &["--write-baseline", "cfg/b.json", "--output-format", "json"],
        );
        assert_eq!(cli.len(), 1, "one finding to baseline: {cli:?}");
        let mut baseline: Value =
            serde_json::from_slice(&std::fs::read(fixture.path("cfg/b.json")).unwrap()).unwrap();
        assert_eq!(
            baseline["entries"][0]["path"],
            json!("cfg/R/diag.R"),
            "keys relative to the working directory: {baseline:?}"
        );
        baseline["entries"][0]["path"] = json!("R/diag.R");
        std::fs::write(
            fixture.path("cfg/b.json"),
            format!("{}\n", serde_json::to_string_pretty(&baseline).unwrap()),
        )
        .unwrap();

        let (mut session, server) = spawn_session(
            &[fixture.root()],
            json!({}),
            Some(json!({
                "settings": [{"configuration": "cfg/ry.toml"}],
                "globalSettings": {}
            })),
        )
        .await;
        let covered_uri = file_uri(&fixture.path("cfg/R/diag.R"));
        let uncovered_uri = file_uri(&fixture.path("R/diag.R"));

        let mark = session.publication_mark();
        session
            .open(&covered_uri, 1, "x <- never_bound_here\n")
            .await
            .unwrap();
        let covered = session
            .published_diagnostics_after(&covered_uri, mark)
            .await
            .unwrap();
        assert_eq!(
            count_code(&covered, "RY010"),
            0,
            "the baselined finding is suppressed under the cfg/ config: {covered}"
        );
        let mark = session.publication_mark();
        session
            .open(&uncovered_uri, 1, "x <- never_bound_here\n")
            .await
            .unwrap();
        let uncovered = session
            .published_diagnostics_after(&uncovered_uri, mark)
            .await
            .unwrap();
        assert_eq!(
            count_code(&uncovered, "RY010"),
            1,
            "the same-shaped finding at the root is not covered: {uncovered}"
        );

        // Move the config to the folder root with an unloadable baseline
        // and switch the `configuration` setting to it.
        fixture
            .write_file("ry.toml", "baseline = \"missing.json\"\n")
            .unwrap();
        sync_barrier(&mut session, &uncovered_uri).await;
        let mark = session.publication_mark();
        session
            .notify(
                "workspace/didChangeConfiguration",
                json!({ "settings": { "configuration": "ry.toml" } }),
            )
            .await
            .unwrap();
        let after = session
            .published_diagnostics_after(&uncovered_uri, mark)
            .await
            .unwrap();
        assert_eq!(
            count_code(&after, "RY010"),
            1,
            "the moved config's failed baseline must be cleared, not retained against the new anchor: {after}"
        );
        join_session(session, server).await;
    })
}
