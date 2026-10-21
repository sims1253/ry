//! End-to-end tests for `ry check --watch`: the process stays alive and
//! file-system changes take effect without a restart.
//!
//! Each step waits for the previous pass's output before the next write, so
//! successive writes land at least one 500 ms poll apart and their mtimes
//! differ. Positive assertions wait on output with a deadline instead of
//! sleeping. The two negative controls sleep one [`NEGATIVE_WINDOW`] and then
//! run a positive control, so a stalled watcher fails instead of passing.

use std::io::{BufRead, BufReader, Read};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// A spawned `ry check --watch` plus its captured pipes. The `Drop` impl
/// kills the child so a failed assertion cannot leak a polling process
/// into the rest of the test run.
struct WatchSession {
    child: Child,
    stdout: Arc<Mutex<String>>,
    stderr: Arc<Mutex<String>>,
}

impl WatchSession {
    /// `root` is the positional check input: a directory to watch, or an
    /// explicit `.R` file whose package ancestors must stay tracked.
    fn spawn(root: &std::path::Path) -> Self {
        Self::spawn_at(None, root)
    }

    /// Like [`WatchSession::spawn`], from `cwd` so `input` can be relative.
    fn spawn_at(cwd: Option<&std::path::Path>, input: &std::path::Path) -> Self {
        let mut command = Command::new(env!("CARGO_BIN_EXE_ry"));
        command
            .arg("check")
            .arg("--watch")
            .arg("--color")
            .arg("never")
            .arg(input)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(dir) = cwd {
            command.current_dir(dir);
        }
        let mut child = command.spawn().expect("failed to spawn ry check --watch");
        let stdout = Arc::new(Mutex::new(String::new()));
        let stderr = Arc::new(Mutex::new(String::new()));
        pump(child.stdout.take(), Arc::clone(&stdout));
        pump(child.stderr.take(), Arc::clone(&stderr));
        Self {
            child,
            stdout,
            stderr,
        }
    }

    fn assert_alive(&mut self, context: &str) {
        assert!(
            self.child
                .try_wait()
                .expect("failed to poll watch process")
                .is_none(),
            "watch process exited early ({context}); stderr: {}",
            self.stderr.lock().unwrap()
        );
    }
}

impl Drop for WatchSession {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Drain `pipe` line by line into `buffer` on a background thread until
/// EOF (the child exiting or being killed).
fn pump(pipe: Option<impl Read + Send + 'static>, buffer: Arc<Mutex<String>>) {
    let mut reader = BufReader::new(pipe.expect("pipe must be piped"));
    std::thread::spawn(move || {
        let mut line = String::new();
        while reader.read_line(&mut line).unwrap_or(0) > 0 {
            buffer.lock().unwrap().push_str(&line);
            line.clear();
        }
    });
}

/// Wait until `buffer` contains `needle`, polling every 100 ms up to a
/// 30 s deadline. Panics with the full captured output on timeout so a
/// slow machine still fails informatively instead of hanging the suite.
fn wait_for(buffer: &Mutex<String>, needle: &str, what: &str) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        if buffer.lock().unwrap().contains(needle) {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!(
        "timed out waiting for {what} ({needle:?}); captured: {}",
        buffer.lock().unwrap()
    );
}

/// Wait until `buffer` contains more occurrences of `needle` than `before`.
/// For phases whose signal is a repeated line (another summary, another
/// screenful) rather than a first-appearing marker: returns once a new
/// pass lands, or panics with the full capture on timeout.
fn wait_for_more(buffer: &Mutex<String>, needle: &str, before: usize, what: &str) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        if buffer.lock().unwrap().matches(needle).count() > before {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!(
        "timed out waiting for {what} (a new {needle:?} past {before}); captured: {}",
        buffer.lock().unwrap()
    );
}

/// Minimal valid package metadata with one import binding.
fn write_package_with_import(pkg: &std::path::Path, name: &str) {
    std::fs::create_dir_all(pkg.join("R")).unwrap();
    std::fs::write(
        pkg.join("DESCRIPTION"),
        format!("Package: {name}\nVersion: 0.0.0.9000\n"),
    )
    .unwrap();
    std::fs::write(pkg.join("NAMESPACE"), "importFrom(shiny,tags)\n").unwrap();
}

/// Occurrence count of `needle` in a pumped buffer. Used for
/// disappearance pins: stdout accumulates every screenful, so a finding
/// going quiet is "the count stops growing", not "the text is gone".
fn occurrences(buffer: &Mutex<String>, needle: &str) -> usize {
    buffer.lock().unwrap().matches(needle).count()
}

/// Quiet window for the negative controls: five polls of the watch loop's
/// 500 ms interval.
const NEGATIVE_WINDOW: Duration = Duration::from_millis(2500);

/// An empty directory enters the watch loop with zero files, and the first
/// created `.R` file is checked.
#[test]
fn watch_stays_alive_on_empty_initial_set_and_picks_up_first_file() {
    let tmp = tempfile::tempdir().unwrap();
    let mut session = WatchSession::spawn(tmp.path());

    // An early exit takes milliseconds, so a process alive after two
    // seconds has entered the watch loop.
    std::thread::sleep(Duration::from_secs(2));
    session.assert_alive("empty initial set");
    wait_for(&session.stderr, "watching 0 file(s)", "watch loop entry");

    // The first created file must trigger a re-check that reports its
    // diagnostic: a bare unbound name is an RY010 warning.
    std::fs::write(
        tmp.path().join("main.R"),
        "result <- genuinely_missing_name\n",
    )
    .unwrap();
    wait_for(&session.stdout, "RY010", "first-file diagnostic");
    assert!(
        session
            .stdout
            .lock()
            .unwrap()
            .contains("genuinely_missing_name"),
        "the re-check must name the new finding: {}",
        session.stdout.lock().unwrap()
    );
    session.assert_alive("after first re-check");
}

/// Config inputs reload mid-watch without an R-file edit: baseline
/// deletion, emptying, and restore, `ry.toml` edits, and a broken `ry.toml`
/// or machine `output-format`, which keep the last good config and warn
/// once per episode.
#[test]
fn watch_reloads_config_inputs() {
    let tmp = tempfile::tempdir().unwrap();
    let finding = "`genuinely_missing_config_name` is not bound";
    std::fs::write(
        tmp.path().join("watched.R"),
        "result <- genuinely_missing_config_name\n",
    )
    .unwrap();
    // Seed the baseline with the config in place and the same absolute
    // input the session gets, so both key entries by the same paths.
    std::fs::write(tmp.path().join("ry.toml"), "baseline = \"baseline.json\"\n").unwrap();
    let baseline = tmp.path().join("baseline.json");
    let seed = ry_testkit::ry!()
        .arg("check")
        .arg("--write-baseline")
        .arg(&baseline)
        .arg(tmp.path())
        .run();
    assert!(seed.success(), "{seed:?}");
    let seeded = std::fs::read_to_string(&baseline).unwrap();
    assert!(seeded.contains("genuinely_missing_config_name"), "{seeded}");

    let mut session = WatchSession::spawn(tmp.path());
    wait_for(&session.stderr, "watching 1 file(s)", "initial pass");
    wait_for(&session.stderr, "checked 1 file(s)", "initial pass summary");
    assert!(
        !session.stdout.lock().unwrap().contains("RY010"),
        "the seeded baseline must accept the finding: {}",
        session.stdout.lock().unwrap()
    );

    // A deleted config-level baseline settles to none, as a fresh run does.
    std::fs::remove_file(&baseline).unwrap();
    wait_for(
        &session.stderr,
        "could not read baseline",
        "deletion warning",
    );
    wait_for(&session.stdout, finding, "post-deletion diagnostic");

    // An empty baseline accepts nothing.
    let found = occurrences(&session.stdout, finding);
    std::fs::write(&baseline, "{\"version\": 1, \"entries\": []}\n").unwrap();
    wait_for_more(&session.stdout, finding, found, "post-emptying pass");

    // The seeded baseline accepts the finding again.
    let quiet = occurrences(&session.stderr, "0 warning(s)");
    std::fs::write(&baseline, &seeded).unwrap();
    wait_for_more(&session.stderr, "0 warning(s)", quiet, "post-restore pass");

    // Dropping the `baseline` key reports the finding again.
    let found = occurrences(&session.stdout, finding);
    std::fs::write(tmp.path().join("ry.toml"), "").unwrap();
    wait_for_more(&session.stdout, finding, found, "post-config-edit pass");

    // A path-scoped ignore quiets it.
    let quiet = occurrences(&session.stderr, "0 warning(s)");
    std::fs::write(
        tmp.path().join("ry.toml"),
        "[[rule-overrides]]\npaths = [\"*.R\"]\nignore = [\"RY010\"]\n",
    )
    .unwrap();
    wait_for_more(&session.stderr, "0 warning(s)", quiet, "post-ignore pass");

    // A broken ry.toml keeps the ignore and warns once, not once per poll.
    let found = occurrences(&session.stdout, finding);
    std::fs::write(tmp.path().join("ry.toml"), "this is not = = valid toml\n").unwrap();
    let broken = "keeping the last good configuration";
    wait_for(&session.stderr, broken, "broken-config warning");
    std::thread::sleep(Duration::from_secs(2));
    assert_eq!(
        occurrences(&session.stderr, broken),
        1,
        "a broken config warns once, not per poll: {}",
        session.stderr.lock().unwrap()
    );
    assert_eq!(
        occurrences(&session.stdout, finding),
        found,
        "the last good config stays in effect: {}",
        session.stdout.lock().unwrap()
    );
    session.assert_alive("while config is broken");

    // Fixing it reloads without the ignore.
    std::fs::write(tmp.path().join("ry.toml"), "").unwrap();
    wait_for(&session.stderr, "parses again", "recovery note");
    wait_for_more(&session.stdout, finding, found, "post-recovery pass");

    // A machine format keeps the human one instead of mixing JSON into the
    // screen clears.
    std::fs::write(tmp.path().join("ry.toml"), "output-format = \"json\"\n").unwrap();
    let machine = "requires the full or concise output format";
    wait_for(&session.stderr, machine, "format-switch warning");
    std::thread::sleep(Duration::from_secs(2));
    assert_eq!(
        occurrences(&session.stderr, machine),
        1,
        "a machine format warns once, not per poll: {}",
        session.stderr.lock().unwrap()
    );
    assert!(
        !session
            .stdout
            .lock()
            .unwrap()
            .contains("\"code\": \"RY010\""),
        "the human format must stay in effect: {}",
        session.stdout.lock().unwrap()
    );

    let recovered = occurrences(&session.stderr, "parses again");
    std::fs::remove_file(tmp.path().join("ry.toml")).unwrap();
    wait_for_more(
        &session.stderr,
        "parses again",
        recovered,
        "format recovery note",
    );
    session.assert_alive("after format recovery");
}

/// Dropping a NAMESPACE import re-checks without an R-file edit, whichever
/// form names the input, and restoring it converges back to quiet. The
/// package-root case deletes NAMESPACE, the others empty it.
#[test]
fn watch_reacts_to_namespace_edits_for_every_input_form() {
    // (form, package dir under the temp root, check input, relative to the
    // temp root as the working directory, delete NAMESPACE instead of
    // emptying it)
    let forms = [
        ("package root", "", "", false, true),
        ("nested package", "pkg", "", false, false),
        ("package subdirectory", "pkg", "pkg/R", false, false),
        ("explicit file", "pkg", "pkg/R/use.R", false, false),
        ("relative file", "pkg", "pkg/R/use.R", true, false),
    ];
    let finding = "`tags` is not bound";
    for (form, package, input, relative, delete) in forms {
        let tmp = tempfile::tempdir().unwrap();
        let pkg = tmp.path().join(package);
        write_package_with_import(&pkg, "namespaceedit");
        std::fs::write(pkg.join("R/use.R"), "page <- tags\n").unwrap();
        let mut session = if relative {
            WatchSession::spawn_at(Some(tmp.path()), std::path::Path::new(input))
        } else {
            WatchSession::spawn(&tmp.path().join(input))
        };
        wait_for(&session.stderr, "watching 1 file(s)", form);
        wait_for(&session.stderr, "checked 1 file(s)", form);
        assert!(
            !session.stdout.lock().unwrap().contains("RY010"),
            "{form}: the initial pass must resolve the import: {}",
            session.stdout.lock().unwrap()
        );

        if delete {
            std::fs::remove_file(pkg.join("NAMESPACE")).unwrap();
        } else {
            std::fs::write(pkg.join("NAMESPACE"), "").unwrap();
        }
        wait_for(&session.stdout, finding, form);

        let found = occurrences(&session.stdout, finding);
        let quiet = occurrences(&session.stderr, "0 warning(s)");
        std::fs::write(pkg.join("NAMESPACE"), "importFrom(shiny,tags)\n").unwrap();
        wait_for_more(&session.stderr, "0 warning(s)", quiet, form);
        assert_eq!(occurrences(&session.stdout, finding), found, "{form}");
        session.assert_alive(form);
    }
}

/// The absent NAMESPACE of a nested package is watched from the start, so
/// creating it binds the import and deleting it unbinds the name again.
#[test]
fn watch_reacts_to_nested_namespace_create_and_delete() {
    let tmp = tempfile::tempdir().unwrap();
    let pkg = tmp.path().join("pkg");
    std::fs::create_dir_all(pkg.join("R")).unwrap();
    std::fs::write(
        pkg.join("DESCRIPTION"),
        "Package: nestedcreate\nVersion: 0.0.0.9000\n",
    )
    .unwrap();
    std::fs::write(pkg.join("R/use.R"), "page <- tags\n").unwrap();
    let mut session = WatchSession::spawn(tmp.path());

    // Without NAMESPACE the name is unbound from the start.
    wait_for(&session.stderr, "watching 1 file(s)", "initial pass");
    wait_for(&session.stdout, "`tags`", "initial unbound diagnostic");

    // Creating the NAMESPACE binds the name: the next pass is quiet.
    let tags_before = occurrences(&session.stdout, "`tags` is not bound");
    let quiet_before = occurrences(&session.stderr, "0 warning(s)");
    std::fs::write(pkg.join("NAMESPACE"), "importFrom(shiny,tags)\n").unwrap();
    wait_for_more(
        &session.stderr,
        "0 warning(s)",
        quiet_before,
        "post-creation quiet pass",
    );
    assert_eq!(
        occurrences(&session.stdout, "`tags` is not bound"),
        tags_before,
        "creating the NAMESPACE must bind the name: {}",
        session.stdout.lock().unwrap()
    );

    // Deleting it unbinds the name again.
    std::fs::remove_file(pkg.join("NAMESPACE")).unwrap();
    wait_for_more(
        &session.stdout,
        "`tags` is not bound",
        tags_before,
        "post-deletion diagnostic",
    );
    session.assert_alive("after NAMESPACE delete");
}

/// Creating a nested DESCRIPTION isolates the sub-package from a top-level
/// file that used its binding, and deleting it merges the scopes back.
#[test]
fn watch_reacts_to_nested_description_creation_and_deletion() {
    let tmp = tempfile::tempdir().unwrap();
    let pkg = tmp.path().join("pkg");
    std::fs::create_dir_all(pkg.join("R")).unwrap();
    std::fs::write(pkg.join("R/defs.R"), "only_in_pkg <- 1L\n").unwrap();
    std::fs::write(tmp.path().join("top.R"), "value <- only_in_pkg\n").unwrap();
    let mut session = WatchSession::spawn(tmp.path());

    // One shared scope: the top-level use resolves the package file's
    // binding, and the absent DESCRIPTION is already watched.
    wait_for(&session.stderr, "watching 2 file(s)", "initial pass");
    assert!(
        !session.stdout.lock().unwrap().contains("RY010"),
        "the initial pass must resolve the shared binding: {}",
        session.stdout.lock().unwrap()
    );

    // Isolate the package: only pkg/DESCRIPTION changes.
    std::fs::write(
        pkg.join("DESCRIPTION"),
        "Package: isolate\nVersion: 0.0.0.9000\n",
    )
    .unwrap();
    wait_for(&session.stdout, "RY010", "post-creation diagnostic");
    assert!(
        session.stdout.lock().unwrap().contains("`only_in_pkg`"),
        "the re-check must flag the now-isolated binding: {}",
        session.stdout.lock().unwrap()
    );

    // Deleting the DESCRIPTION merges the scopes back: the next pass is
    // quiet.
    let finding_before = occurrences(&session.stdout, "`only_in_pkg` is not bound");
    let quiet_before = occurrences(&session.stderr, "0 warning(s)");
    std::fs::remove_file(pkg.join("DESCRIPTION")).unwrap();
    wait_for_more(
        &session.stderr,
        "0 warning(s)",
        quiet_before,
        "post-deletion quiet pass",
    );
    assert_eq!(
        occurrences(&session.stdout, "`only_in_pkg` is not bound"),
        finding_before,
        "deleting the DESCRIPTION must re-merge the scopes: {}",
        session.stdout.lock().unwrap()
    );
    session.assert_alive("after DESCRIPTION delete");
}

/// Editing one package's NAMESPACE lands exactly one pass, which reprints the
/// sibling's finding once.
#[test]
fn watch_sibling_package_metadata_edits_stay_scoped() {
    let tmp = tempfile::tempdir().unwrap();
    let pkga = tmp.path().join("pkga");
    let pkgb = tmp.path().join("pkgb");
    write_package_with_import(&pkga, "siblinga");
    std::fs::create_dir_all(pkgb.join("R")).unwrap();
    std::fs::write(
        pkgb.join("DESCRIPTION"),
        "Package: siblingb\nVersion: 0.0.0.9000\n",
    )
    .unwrap();
    std::fs::write(pkga.join("R/use.R"), "page <- tags\n").unwrap();
    std::fs::write(pkgb.join("R/b.R"), "b_value <- missing_in_b\n").unwrap();
    let mut session = WatchSession::spawn(tmp.path());

    // pkgb's unbound name is flagged from the start; pkga's import
    // resolves.
    wait_for(&session.stderr, "watching 2 file(s)", "initial pass");
    wait_for(
        &session.stdout,
        "`missing_in_b`",
        "initial sibling diagnostic",
    );

    let sibling_before = occurrences(&session.stdout, "`missing_in_b` is not bound");
    let passes_before = occurrences(&session.stderr, "checked 2 file(s)");
    std::fs::write(pkga.join("NAMESPACE"), "").unwrap();
    // A pass writes stdout first and its stderr summary last, so waiting
    // for the stdout lines and then the summary means the count is final.
    wait_for_more(
        &session.stdout,
        "`missing_in_b` is not bound",
        sibling_before,
        "post-edit diagnostic",
    );
    wait_for(&session.stdout, "`tags`", "post-edit pkga diagnostic");
    wait_for_more(
        &session.stderr,
        "checked 2 file(s)",
        passes_before,
        "post-edit completion summary",
    );
    assert_eq!(
        occurrences(&session.stdout, "`missing_in_b` is not bound"),
        sibling_before + 1,
        "one pass must reprint pkgb's finding exactly once: {}",
        session.stdout.lock().unwrap()
    );
    session.assert_alive("after sibling metadata edit");
}

/// An empty workspace picks up its first file and then tracks the package
/// that grows around it: the DESCRIPTION re-checks, still flagged, and the
/// NAMESPACE binds the import.
#[test]
fn watch_empty_workspace_gains_package_and_metadata() {
    let tmp = tempfile::tempdir().unwrap();
    let pkg = tmp.path().join("pkg");
    let mut session = WatchSession::spawn(tmp.path());
    wait_for(&session.stderr, "watching 0 file(s)", "watch loop entry");

    // The first source file: unbound, as a fresh check would report.
    std::fs::create_dir_all(pkg.join("R")).unwrap();
    std::fs::write(pkg.join("R/use.R"), "page <- tags\n").unwrap();
    wait_for(&session.stdout, "`tags`", "first-file diagnostic");

    // The DESCRIPTION lands one pass that still flags the name.
    let tags_after_source = occurrences(&session.stdout, "`tags` is not bound");
    let passes_before = occurrences(&session.stderr, "checked 1 file(s)");
    std::fs::write(
        pkg.join("DESCRIPTION"),
        "Package: growaround\nVersion: 0.0.0.9000\n",
    )
    .unwrap();
    // Stdout lines first, then the summary, as in the sibling test.
    wait_for_more(
        &session.stdout,
        "`tags` is not bound",
        tags_after_source,
        "post-DESCRIPTION diagnostic",
    );
    wait_for_more(
        &session.stderr,
        "checked 1 file(s)",
        passes_before,
        "post-DESCRIPTION completion summary",
    );
    assert_eq!(
        occurrences(&session.stdout, "`tags` is not bound"),
        tags_after_source + 1,
        "grouping alone must not bind the imported name: {}",
        session.stdout.lock().unwrap()
    );

    // The NAMESPACE creation binds the import: the quiet pass adds no
    // new occurrence beyond the two flagged passes already printed.
    let quiet_before = occurrences(&session.stderr, "0 warning(s)");
    std::fs::write(pkg.join("NAMESPACE"), "importFrom(shiny,tags)\n").unwrap();
    wait_for_more(
        &session.stderr,
        "0 warning(s)",
        quiet_before,
        "post-NAMESPACE quiet pass",
    );
    assert_eq!(
        occurrences(&session.stdout, "`tags` is not bound"),
        tags_after_source + 1,
        "the import must now bind the name: {}",
        session.stdout.lock().unwrap()
    );
    session.assert_alive("after package growth");
}

/// A rename keeps the file's mtime, so moving a definition between packages
/// re-checks through the file-set change and the re-derived metadata.
#[test]
fn watch_moving_source_between_packages_converges() {
    let tmp = tempfile::tempdir().unwrap();
    let pkga = tmp.path().join("pkga");
    let pkgb = tmp.path().join("pkgb");
    for (root, name) in [(&pkga, "movea"), (&pkgb, "moveb")] {
        std::fs::create_dir_all(root.join("R")).unwrap();
        std::fs::write(
            root.join("DESCRIPTION"),
            format!("Package: {name}\nVersion: 0.0.0.9000\n"),
        )
        .unwrap();
    }
    std::fs::write(pkga.join("R/a.R"), "shared_val <- 1L\n").unwrap();
    std::fs::write(pkgb.join("R/b.R"), "take <- shared_val\n").unwrap();
    let mut session = WatchSession::spawn(tmp.path());

    // Cross-package use: unbound.
    wait_for(&session.stderr, "watching 2 file(s)", "initial pass");
    wait_for(
        &session.stdout,
        "`shared_val`",
        "initial cross-package diagnostic",
    );
    let finding = "`shared_val` is not bound";

    // Move the definition into pkgb: same package now, binding resolves.
    let finding_before = occurrences(&session.stdout, finding);
    let quiet_before = occurrences(&session.stderr, "0 warning(s)");
    std::fs::rename(pkga.join("R/a.R"), pkgb.join("R/a.R")).unwrap();
    wait_for_more(
        &session.stderr,
        "0 warning(s)",
        quiet_before,
        "post-move quiet pass",
    );
    assert_eq!(
        occurrences(&session.stdout, finding),
        finding_before,
        "the moved definition must resolve the use: {}",
        session.stdout.lock().unwrap()
    );

    // Move it back: the use is cross-package again and re-flags.
    std::fs::rename(pkgb.join("R/a.R"), pkga.join("R/a.R")).unwrap();
    wait_for_more(
        &session.stdout,
        finding,
        finding_before,
        "post-move-back diagnostic",
    );
    session.assert_alive("after source move back");
}

/// Dropping an exclude admits a package's file, and that package's
/// NAMESPACE becomes a watched input.
#[test]
fn watch_config_admission_tracks_new_metadata_dependencies() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("ry.toml"), "exclude = [\"**/pkg2/**\"]\n").unwrap();
    std::fs::write(tmp.path().join("main.R"), "top <- 1L\n").unwrap();
    let pkg2 = tmp.path().join("pkg2");
    write_package_with_import(&pkg2, "admitted");
    std::fs::write(pkg2.join("R/use.R"), "page <- tags\n").unwrap();
    let mut session = WatchSession::spawn(tmp.path());

    // pkg2 is excluded: one file, and nothing from it is reported.
    wait_for(&session.stderr, "watching 1 file(s)", "initial pass");
    assert!(
        !session.stdout.lock().unwrap().contains("RY010"),
        "the excluded package must not be checked: {}",
        session.stdout.lock().unwrap()
    );

    // Drop the exclude: the reload re-scans and admits pkg2's file; its
    // NAMESPACE import resolves, so the pass stays quiet.
    std::fs::write(tmp.path().join("ry.toml"), "").unwrap();
    wait_for(&session.stderr, "checked 2 file(s)", "post-admission pass");
    assert!(
        !session.stdout.lock().unwrap().contains("RY010"),
        "the admitted package's import must resolve: {}",
        session.stdout.lock().unwrap()
    );

    // The admitted package's metadata is now a tracked dependency.
    std::fs::write(pkg2.join("NAMESPACE"), "").unwrap();
    wait_for(&session.stdout, "RY010", "post-admission metadata edit");
    assert!(
        session.stdout.lock().unwrap().contains("`tags`"),
        "the re-check must flag the no-longer-imported name: {}",
        session.stdout.lock().unwrap()
    );
    session.assert_alive("after admitted metadata edit");
}

/// Negative control: metadata of a sibling tree with no discovered R file
/// does not trigger a pass.
#[test]
fn watch_ignores_unrelated_package_metadata() {
    let tmp = tempfile::tempdir().unwrap();
    let pkg = tmp.path().join("pkg");
    write_package_with_import(&pkg, "mainpkg");
    std::fs::write(pkg.join("R/use.R"), "page <- tags\n").unwrap();
    let mut session = WatchSession::spawn(tmp.path());

    wait_for(&session.stderr, "watching 1 file(s)", "initial pass");
    // The initial summary can arrive after the "watching" line.
    wait_for(&session.stderr, "checked 1 file(s)", "initial pass summary");
    assert!(
        !session.stdout.lock().unwrap().contains("RY010"),
        "the initial pass must resolve the imported name: {}",
        session.stdout.lock().unwrap()
    );

    // Count passes before the writes so an extra pass cannot hide in the
    // baseline.
    let passes_before = occurrences(&session.stderr, "checked 1 file(s)");
    let unrelated = tmp.path().join("unrelated");
    std::fs::create_dir_all(&unrelated).unwrap();
    std::fs::write(
        unrelated.join("DESCRIPTION"),
        "Package: unrelated\nVersion: 0.0.0.9000\n",
    )
    .unwrap();
    std::fs::write(unrelated.join("NAMESPACE"), "importFrom(shiny,tags)\n").unwrap();

    std::thread::sleep(NEGATIVE_WINDOW);
    assert_eq!(
        occurrences(&session.stderr, "checked 1 file(s)"),
        passes_before,
        "unrelated metadata must not trigger a re-check: {}",
        session.stderr.lock().unwrap()
    );
    assert!(
        !session.stdout.lock().unwrap().contains("RY010"),
        "unrelated metadata must not change the findings: {}",
        session.stdout.lock().unwrap()
    );

    // Positive control: the watched package's own metadata re-checks.
    std::fs::write(pkg.join("NAMESPACE"), "").unwrap();
    wait_for(&session.stdout, "RY010", "control diagnostic");
    assert!(
        session.stdout.lock().unwrap().contains("`tags`"),
        "the control re-check must flag the no-longer-imported name: {}",
        session.stdout.lock().unwrap()
    );
    session.assert_alive("after control metadata edit");
}

/// Negative control: metadata above the watched package's DESCRIPTION does
/// not trigger a pass, because a fresh check groups each file by its
/// nearest DESCRIPTION.
#[test]
fn watch_ignores_metadata_above_absorbing_boundary() {
    let tmp = tempfile::tempdir().unwrap();
    let pkg = tmp.path().join("proj");
    write_package_with_import(&pkg, "boundary");
    std::fs::write(pkg.join("R/use.R"), "page <- tags\n").unwrap();
    let mut session = WatchSession::spawn(&pkg);

    // The root's own metadata resolves the import from the start.
    wait_for(&session.stderr, "watching 1 file(s)", "initial pass");
    // The initial summary can arrive after the "watching" line.
    wait_for(&session.stderr, "checked 1 file(s)", "initial pass summary");
    assert!(
        !session.stdout.lock().unwrap().contains("RY010"),
        "the initial pass must resolve the imported name: {}",
        session.stdout.lock().unwrap()
    );

    // Count passes before the writes so an extra pass cannot hide in the
    // baseline.
    let passes_before = occurrences(&session.stderr, "checked 1 file(s)");
    std::fs::write(
        tmp.path().join("DESCRIPTION"),
        "Package: above\nVersion: 0.0.0.9000\n",
    )
    .unwrap();
    std::fs::write(tmp.path().join("NAMESPACE"), "importFrom(shiny,tags)\n").unwrap();

    std::thread::sleep(NEGATIVE_WINDOW);
    assert_eq!(
        occurrences(&session.stderr, "checked 1 file(s)"),
        passes_before,
        "metadata above the absorbing boundary must not trigger a re-check: {}",
        session.stderr.lock().unwrap()
    );
    assert!(
        !session.stdout.lock().unwrap().contains("RY010"),
        "metadata above the absorbing boundary must not change the findings: {}",
        session.stdout.lock().unwrap()
    );
    session.assert_alive("after above-boundary metadata creation");

    // Positive control: the boundary's own metadata re-checks.
    std::fs::write(pkg.join("NAMESPACE"), "").unwrap();
    wait_for(&session.stdout, "RY010", "post-boundary-edit diagnostic");
    assert!(
        session.stdout.lock().unwrap().contains("`tags`"),
        "the re-check must flag the no-longer-imported name: {}",
        session.stdout.lock().unwrap()
    );
    session.assert_alive("after boundary NAMESPACE edit");
}
