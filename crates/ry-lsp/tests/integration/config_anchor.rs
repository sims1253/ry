//! Config-relative paths (`exclude`, `include-build-ignored`, baseline keys)
//! anchor at the directory of the `ry.toml` that was loaded, not at the
//! workspace folder, as `ry check` does.

use crate::harness;

use harness::{cli_diagnostics, count_code, file_uri, join_session, spawn_session, sync_barrier};
use ry_testkit::FixtureProject;
use serde_json::{Value, json};

/// `exclude` covers `pkgA/R/generated/gen.R` and not the uncovered file,
/// whichever folder layout or `configuration` setting locates the config.
#[tokio::test]
async fn exclude_anchors_at_the_config_directory() {
    const SOURCE: &str = "x <- never_bound_here\n";
    let parent = ("ry.toml", "exclude = [\"pkgA/R/generated/**\"]\n");
    let external = json!({
        "settings": [{"configuration": "../ry.toml"}],
        "globalSettings": {}
    });
    // (name, config, folders, initialization options, uncovered file)
    let cases = [
        (
            "parent config",
            parent,
            &["pkgA"][..],
            None,
            "pkgA/R/keep.R",
        ),
        (
            "in-folder config",
            ("pkgA/ry.toml", "exclude = [\"R/generated/**\"]\n"),
            &["pkgA"][..],
            None,
            "pkgA/R/keep.R",
        ),
        // The `..` segment also checks that the anchor is normalized: the
        // raw `pkgA/..` never prefix-matches the clean paths diagnostics
        // carry.
        (
            "external configuration setting",
            parent,
            &["pkgA"][..],
            Some(external),
            "pkgA/R/keep.R",
        ),
        (
            "sibling folders sharing the parent config",
            parent,
            &["pkgA", "pkgB"][..],
            None,
            "pkgB/R/keep.R",
        ),
    ];
    for (name, (config_path, config), folders, options, keep) in cases {
        let fixture = FixtureProject::empty().unwrap();
        fixture.write_file(config_path, config).unwrap();
        fixture
            .write_file("pkgA/R/generated/gen.R", SOURCE)
            .unwrap();
        fixture.write_file(keep, SOURCE).unwrap();
        // `ry check <folder>` from the repository is the oracle.
        let cli: Vec<_> = folders
            .iter()
            .flat_map(|folder| cli_diagnostics(fixture.root(), folder, &[]))
            .collect();
        assert!(
            cli.iter().any(|d| d.path == keep),
            "{name}: the CLI reports {keep}: {cli:?}"
        );
        assert!(
            !cli.iter().any(|d| d.path.contains("gen.R")),
            "{name}: the CLI excludes gen.R: {cli:?}"
        );

        let folders: Vec<_> = folders.iter().map(|folder| fixture.path(folder)).collect();
        let roots: Vec<_> = folders.iter().map(|folder| folder.as_path()).collect();
        let (mut session, server) = spawn_session(&roots, json!({}), options).await;
        let gen_uri = file_uri(&fixture.path("pkgA/R/generated/gen.R"));
        let gen_publish = harness::open_published(&mut session, &gen_uri, 1, SOURCE).await;
        assert_eq!(
            count_code(&gen_publish, "RY010"),
            0,
            "{name}: gen.R must be excluded: {gen_publish}"
        );
        let keep_uri = file_uri(&fixture.path(keep));
        let keep_publish = harness::open_published(&mut session, &keep_uri, 1, SOURCE).await;
        assert_eq!(
            count_code(&keep_publish, "RY010"),
            1,
            "{name}: {keep} is not covered and must still report: {keep_publish}"
        );
        join_session(session, server).await;
    }
}

/// A CLI-generated baseline uses config-directory-relative keys, so a
/// `ry.toml` above the folder must anchor key normalization at its own
/// directory or every baselined finding reappears in the editor.
#[tokio::test]
async fn parent_config_baseline_keys_match_in_the_editor() {
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
    let cli = cli_diagnostics(
        fixture.root(),
        "pkgA",
        &["--write-baseline", "baseline.json"],
    );
    assert!(
        cli.iter().any(|d| d.code == "RY010"),
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

    let diag_publish =
        harness::open_published(&mut session, &diag_uri, 1, "x <- never_bound_here\n").await;
    assert_eq!(
        count_code(&diag_publish, "RY010"),
        0,
        "the baselined finding must stay baselined when the config sits above the folder: {diag_publish}"
    );

    let other_publish =
        harness::open_published(&mut session, &other_uri, 1, "z <- length(xx = 1L)\n").await;
    assert_eq!(
        count_code(&other_publish, "RY090"),
        1,
        "an unbaselined finding in the same folder must still publish: {other_publish}"
    );
    join_session(session, server).await;
}

/// A parent config's `include-build-ignored` brings a package's
/// `.Rbuildignore`d `vendor/` tree into the index, so the caller's binding
/// resolves. Without the include, the binding stays unresolved. The caller
/// is a script outside `R/`: indexed vendor code never joins the package
/// namespace that `R/` code resolves against (#804).
#[tokio::test]
async fn parent_config_include_build_ignored_reindexes_vendor_code() {
    for (config, unresolved) in [
        ("include-build-ignored = [\"pkgA/vendor/**\"]\n", 0),
        ("", 1),
    ] {
        let fixture = FixtureProject::empty().unwrap();
        fixture.write_file("ry.toml", config).unwrap();
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
            .write_file("pkgA/inst/main.R", "x <- answer + 1L\n")
            .unwrap();
        let folder = fixture.path("pkgA");
        let (mut session, server) = spawn_session(&[&folder], json!({}), None).await;
        let main_uri = file_uri(&fixture.path("pkgA/inst/main.R"));
        sync_barrier(&mut session, &main_uri).await;
        let publish =
            harness::open_published(&mut session, &main_uri, 1, "x <- answer + 1L\n").await;
        assert_eq!(
            count_code(&publish, "RY010"),
            unresolved,
            "{config:?}: {publish}"
        );
        join_session(session, server).await;
    }
}

/// A retained baseline must stay paired with the anchor it was loaded
/// under. When the config moves to another directory mid-session and the
/// new baseline fails to load, retaining the old baseline would pair its
/// old-relative keys with the new anchor: a file that merely shares the
/// key shape (`R/diag.R` under the new root versus `R/diag.R` under the
/// config subdirectory) would be silently absorbed. The reload must
/// clear the baseline instead.
#[tokio::test]
async fn retained_baseline_stays_paired_with_its_config_origin() {
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
    let cli = cli_diagnostics(fixture.root(), "cfg", &["--write-baseline", "cfg/b.json"]);
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

    let covered =
        harness::open_published(&mut session, &covered_uri, 1, "x <- never_bound_here\n").await;
    assert_eq!(
        count_code(&covered, "RY010"),
        0,
        "the baselined finding is suppressed under the cfg/ config: {covered}"
    );
    let uncovered =
        harness::open_published(&mut session, &uncovered_uri, 1, "x <- never_bound_here\n").await;
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
}
