//! End-to-end tests for `ry check --watch` process lifetime and reactivity.
//!
//! These tests drive the real `ry` binary as a long-lived child process
//! against temporary project trees: they pin that watch mode stays alive
//! across input states a one-shot run would exit on, and that file-system
//! changes take effect without a restart. Waits are deadline-based rather
//! than fixed sleeps where the process signals readiness on a pipe, so a
//! loaded CI machine makes the tests slower rather than flaky. Only the
//! pre-fix early-exit grace period is a fixed sleep: the buggy code path
//! exits in milliseconds, so any process alive after it proves the loop
//! was entered.

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

    /// Like [`WatchSession::spawn`], with an explicit working directory
    /// (`None` inherits the test runner's) so the check input can be a
    /// path RELATIVE to it — the form a user gets from running
    /// `ry check pkg/R/use.R` inside the parent directory.
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

/// `ry check --watch` on an empty directory used to print the empty
/// report and exit 0 without ever entering the watch loop, so the first
/// created `.R` file went unobserved until a manual re-run (#529). The
/// process must instead stay alive with zero files and report the first
/// file's diagnostics once it appears. The non-watch empty behavior —
/// the machine-readable empty report, the stderr note, the exit code —
/// is pinned by `empty_discovery_preserves_output_format_contracts` and
/// must stay exactly as it is.
#[test]
fn watch_stays_alive_on_empty_initial_set_and_picks_up_first_file() {
    let tmp = tempfile::tempdir().unwrap();
    let mut session = WatchSession::spawn(tmp.path());

    // Grace period: the pre-fix early return exits in milliseconds, so a
    // process alive after two seconds has entered the watch loop.
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

/// A `ry.toml` edit mid-watch takes effect without touching an R file:
/// ignoring RY010 quiets the finding, un-ignoring it brings the finding
/// back (#530). Appearance-awaiting works on the accumulating pipe;
/// quieting is asserted after the reload lands (see below).
#[test]
fn watch_reloads_config_without_r_file_touch() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("ry.toml"), "ignore = [\"RY010\"]\n").unwrap();
    std::fs::write(
        tmp.path().join("watched.R"),
        "result <- genuinely_missing_config_name\n",
    )
    .unwrap();
    let mut session = WatchSession::spawn(tmp.path());

    // The initial pass runs under the ignore: RY010 never reaches stdout.
    wait_for(&session.stderr, "watching 1 file(s)", "initial pass");
    assert!(
        !session.stdout.lock().unwrap().contains("RY010"),
        "the initial pass must honor the ignore: {}",
        session.stdout.lock().unwrap()
    );

    // Drop the ignore: the next pass must report the finding although no
    // R file changed.
    std::fs::write(tmp.path().join("ry.toml"), "").unwrap();
    wait_for(&session.stdout, "RY010", "post-reload diagnostic");
    assert!(
        session
            .stdout
            .lock()
            .unwrap()
            .contains("genuinely_missing_config_name"),
        "the re-check must name the un-ignored finding: {}",
        session.stdout.lock().unwrap()
    );
    session.assert_alive("after config reload");
}

/// A regenerated baseline mid-watch takes effect on the next pass: the
/// accepted finding disappears from output without touching an R file
/// (#530). The baseline stores counts per (path, code, message), so the
/// regenerated file must match the reported finding exactly — it is
/// produced here by `--write-baseline`, the same way a user would.
#[test]
fn watch_reloads_baseline_without_r_file_touch() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(
        tmp.path().join("watched.R"),
        "result <- genuinely_missing_baseline_name\n",
    )
    .unwrap();
    // Seed the baseline from the current diagnostics via a one-shot run,
    // the same way a user would (`--write-baseline` snapshots what the
    // run reports). The config must exist BEFORE the seed run so the
    // seed resolves the same config root (and the same repo-relative
    // baseline paths) the watch session will use: without it the seed
    // keys entries by absolute path and the watch pass never subtracts
    // them. The seed passes the fixture as an absolute input path, the
    // same form `WatchSession` hands the watch child, so the seed and
    // the watch pass build the same diagnostic paths. The baseline
    // destination must be absolute: the runner's working directory is
    // elsewhere. RY010 is a warning, so the seeding run exits 0 (do NOT
    // assert exit 1); the file it writes is what matters.
    std::fs::write(tmp.path().join("ry.toml"), "baseline = \"baseline.json\"\n").unwrap();
    let baseline_path = tmp.path().join("baseline.json");
    let seed = std::process::Command::new(env!("CARGO_BIN_EXE_ry"))
        .arg("check")
        .arg("--write-baseline")
        .arg(&baseline_path)
        .arg(tmp.path())
        .output()
        .expect("failed to seed baseline");
    assert!(seed.status.success(), "{seed:?}");
    assert!(
        baseline_path.is_file(),
        "the seed run must write the baseline: {seed:?}"
    );
    let seeded = std::fs::read_to_string(&baseline_path).unwrap();
    assert!(
        !seeded.is_empty(),
        "the seeded baseline must be non-empty: {seed:?}"
    );
    assert!(
        seeded.contains("\"code\": \"RY010\""),
        "the seeded baseline must record the RY010 entry: {seeded}"
    );
    assert!(
        seeded.contains("genuinely_missing_baseline_name"),
        "the seeded baseline must accept the finding: {seeded}"
    );

    let mut session = WatchSession::spawn(tmp.path());

    // The initial pass subtracts the seeded baseline: nothing reported.
    wait_for(&session.stderr, "watching 1 file(s)", "initial pass");
    assert!(
        !session.stdout.lock().unwrap().contains("RY010"),
        "the initial pass must subtract the seeded baseline: {}",
        session.stdout.lock().unwrap()
    );

    // Deleting the config-level baseline while its entries still accept
    // the finding settles to no baseline (like a fresh run, which warns
    // and continues without it) rather than keeping the stale
    // acceptances: the finding reappears although no R file changed.
    // Under the old keep-last-good behavior this wait times out, so the
    // phase pins the settle instead of the harness.
    std::fs::remove_file(tmp.path().join("baseline.json")).unwrap();
    wait_for(
        &session.stderr,
        "could not read baseline",
        "deletion warning",
    );
    wait_for(&session.stdout, "RY010", "post-deletion diagnostic");
    assert!(
        session
            .stdout
            .lock()
            .unwrap()
            .contains("genuinely_missing_baseline_name"),
        "deleting the baseline must un-accept the finding: {}",
        session.stdout.lock().unwrap()
    );
    session.assert_alive("after baseline deletion");

    // Empty the baseline (accept nothing): the finding stays reported
    // across the reload although no R file changed. The pin is a fresh
    // pass (a new summary line): there is deliberately no recovery
    // note — the deletion was a settle, not a broken episode.
    let summaries_before = session
        .stderr
        .lock()
        .unwrap()
        .matches("checked 1 file(s)")
        .count();
    std::fs::write(
        tmp.path().join("baseline.json"),
        "{\"version\": 1, \"entries\": []}\n",
    )
    .unwrap();
    wait_for_more(
        &session.stderr,
        "checked 1 file(s)",
        summaries_before,
        "post-emptying pass",
    );
    assert!(
        session.stdout.lock().unwrap().contains("RY010"),
        "the emptied baseline must keep reporting the finding: {}",
        session.stdout.lock().unwrap()
    );
    session.assert_alive("after baseline emptying");

    // Restoring the seeded baseline re-accepts the finding: the next
    // pass goes quiet although no R file changed. There is deliberately
    // no recovery note (deletion was a settle, not a broken episode),
    // so the pin is the next zero-warning summary line.
    let quiet_before = session
        .stderr
        .lock()
        .unwrap()
        .matches("0 warning(s)")
        .count();
    std::fs::write(tmp.path().join("baseline.json"), &seeded).unwrap();
    wait_for_more(
        &session.stderr,
        "0 warning(s)",
        quiet_before,
        "post-restore quiet pass",
    );
    session.assert_alive("after baseline reload");
}

/// A broken `ry.toml` mid-watch keeps the last-good configuration with
/// exactly one warning — no dead session, no per-poll spam — and
/// recovers when the file parses again (#530).
#[test]
fn watch_keeps_last_good_config_on_parse_error_and_recovers() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("ry.toml"), "ignore = [\"RY010\"]\n").unwrap();
    std::fs::write(
        tmp.path().join("watched.R"),
        "result <- genuinely_missing_recovery_name\n",
    )
    .unwrap();
    let mut session = WatchSession::spawn(tmp.path());
    wait_for(&session.stderr, "watching 1 file(s)", "initial pass");

    // Break the config: one warning, last-good behavior (still quiet).
    std::fs::write(tmp.path().join("ry.toml"), "this is not = = valid toml\n").unwrap();
    wait_for(
        &session.stderr,
        "keeping the last good configuration",
        "broken-config warning",
    );
    // Let several polls elapse, then confirm the warning fired exactly
    // once and the finding stayed suppressed under last-good inputs.
    std::thread::sleep(Duration::from_secs(2));
    let stderr = session.stderr.lock().unwrap().clone();
    assert_eq!(
        stderr
            .matches("keeping the last good configuration")
            .count(),
        1,
        "a broken config must warn once per episode, not per poll: {stderr}"
    );
    assert!(
        !session.stdout.lock().unwrap().contains("RY010"),
        "last-good inputs must stay in effect while broken: {}",
        session.stdout.lock().unwrap()
    );
    session.assert_alive("while config is broken");
    drop(stderr);

    // Fix the config with the ignore removed: recovery reloads and the
    // finding appears.
    std::fs::write(tmp.path().join("ry.toml"), "").unwrap();
    wait_for(&session.stderr, "parses again", "recovery note");
    wait_for(&session.stdout, "RY010", "post-recovery diagnostic");
    session.assert_alive("after recovery");
}

/// A mid-watch `ry.toml` switch to a machine-readable `output-format`
/// keeps the last-good human format with one warning instead of
/// interleaving JSON with the loop's screen clears: the startup guard
/// (`--watch requires the full or concise output format`) runs only
/// once, so the reload must enforce the same invariant (#530).
#[test]
fn watch_keeps_human_format_on_machine_format_switch() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(
        tmp.path().join("watched.R"),
        "result <- genuinely_missing_format_name\n",
    )
    .unwrap();
    let mut session = WatchSession::spawn(tmp.path());
    wait_for(&session.stderr, "watching 1 file(s)", "initial pass");
    wait_for(&session.stdout, "RY010", "initial diagnostic");

    // Switch to a machine format: one warning, last-good format kept.
    std::fs::write(tmp.path().join("ry.toml"), "output-format = \"json\"\n").unwrap();
    wait_for(
        &session.stderr,
        "requires the full or concise output format",
        "format-switch warning",
    );
    // Let several polls elapse, then confirm the warning fired exactly
    // once and the human rendering (not JSON) is still in effect.
    std::thread::sleep(Duration::from_secs(2));
    let stderr = session.stderr.lock().unwrap().clone();
    assert_eq!(
        stderr
            .matches("requires the full or concise output format")
            .count(),
        1,
        "a machine format must warn once per episode, not per poll: {stderr}"
    );
    drop(stderr);
    assert!(
        !session
            .stdout
            .lock()
            .unwrap()
            .contains("\"code\": \"RY010\""),
        "the last-good human format must stay in effect: {}",
        session.stdout.lock().unwrap()
    );
    session.assert_alive("after format-switch attempt");

    // Removing the config recovers the default human format with the
    // usual recovery note.
    std::fs::remove_file(tmp.path().join("ry.toml")).unwrap();
    wait_for(&session.stderr, "parses again", "format recovery note");
    session.assert_alive("after format recovery");
}

/// Removing NAMESPACE mid-watch re-resolves the package: a name the
/// `importFrom` used to bind becomes an unbound-variable finding
/// without touching an R file (#530).
#[test]
fn watch_reacts_to_namespace_removal() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir(tmp.path().join("R")).unwrap();
    std::fs::write(
        tmp.path().join("DESCRIPTION"),
        "Package: namespacefixture\nVersion: 0.0.0.9000\n",
    )
    .unwrap();
    std::fs::write(tmp.path().join("NAMESPACE"), "importFrom(shiny,tags)\n").unwrap();
    std::fs::write(tmp.path().join("R/use.R"), "page <- tags\n").unwrap();
    let mut session = WatchSession::spawn(tmp.path());

    // The initial pass resolves `tags` through the NAMESPACE import.
    wait_for(&session.stderr, "watching 1 file(s)", "initial pass");
    assert!(
        !session.stdout.lock().unwrap().contains("tags"),
        "the initial pass must resolve the imported name: {}",
        session.stdout.lock().unwrap()
    );

    // Deleting NAMESPACE must trigger a re-check that reports `tags` as
    // unbound although no R file changed.
    std::fs::remove_file(tmp.path().join("NAMESPACE")).unwrap();
    wait_for(&session.stdout, "RY010", "post-removal diagnostic");
    assert!(
        session.stdout.lock().unwrap().contains("`tags`"),
        "the re-check must flag the no-longer-imported name: {}",
        session.stdout.lock().unwrap()
    );
    session.assert_alive("after NAMESPACE removal");
}

// The nested-package tests below pin that `ry check --watch` tracks the
// DESCRIPTION/NAMESPACE of packages nested BELOW the watched root, not
// just the root's own metadata: a fresh one-shot resolves every
// discovered file against its nearest DESCRIPTION ancestor, so the
// watch result must converge with a fresh check after a metadata-only
// edit, with no R-file touch and no process restart. Distinct
// timestamps come from the harness's phase discipline, not from sleeps
// as assertions: every modification waits for the previous pass's
// observable output first, so successive writes to the same metadata
// file are separated by at least one 500 ms poll and their mtimes
// (nanosecond resolution) always differ.
//
// Shared fixture: a valid DESCRIPTION plus a NAMESPACE importing one
// name (`tags` from shiny, resolvable from the bundled stubs without an
// R installation). Tests that need the import absent write their own
// NAMESPACE or create it later.

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

/// Emptying a nested NAMESPACE mid-watch re-resolves the package: the
/// name the `importFrom` used to bind becomes an unbound-variable
/// finding without any R file being touched, and restoring the import
/// converges back to the fresh-check result (quiet).
#[test]
fn watch_reacts_to_nested_namespace_edit() {
    let tmp = tempfile::tempdir().unwrap();
    let pkg = tmp.path().join("pkg");
    write_package_with_import(&pkg, "nestededit");
    std::fs::write(pkg.join("R/use.R"), "page <- tags\n").unwrap();
    let mut session = WatchSession::spawn(tmp.path());

    // The initial pass resolves `tags` through the nested import.
    wait_for(&session.stderr, "watching 1 file(s)", "initial pass");
    assert!(
        !session.stdout.lock().unwrap().contains("RY010"),
        "the initial pass must resolve the imported name: {}",
        session.stdout.lock().unwrap()
    );

    // Drop the import: only pkg/NAMESPACE changes.
    std::fs::write(pkg.join("NAMESPACE"), "").unwrap();
    wait_for(&session.stdout, "RY010", "post-edit diagnostic");
    assert!(
        session.stdout.lock().unwrap().contains("`tags`"),
        "the re-check must flag the no-longer-imported name: {}",
        session.stdout.lock().unwrap()
    );

    // Restore the import: the next pass goes quiet again.
    let tags_before = occurrences(&session.stdout, "`tags` is not bound");
    let quiet_before = occurrences(&session.stderr, "0 warning(s)");
    std::fs::write(pkg.join("NAMESPACE"), "importFrom(shiny,tags)\n").unwrap();
    wait_for_more(
        &session.stderr,
        "0 warning(s)",
        quiet_before,
        "post-restore quiet pass",
    );
    assert_eq!(
        occurrences(&session.stdout, "`tags` is not bound"),
        tags_before,
        "restoring the import must not re-report the finding: {}",
        session.stdout.lock().unwrap()
    );
    session.assert_alive("after NAMESPACE restore");
}

/// Creating a nested NAMESPACE mid-watch binds the imported name, and
/// deleting it unbinds the name again — both without any R file being
/// touched. The absent NAMESPACE is watched from session start, so the
/// creation registers like an edit.
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

/// Creating a nested DESCRIPTION mid-watch isolates the sub-package: a
/// top-level file that used to share the workspace scope loses the
/// binding defined inside the package, without any source byte
/// changing. Deleting the DESCRIPTION merges the scopes back.
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

/// Watching a package SUBDIRECTORY (here `pkg/R`) keeps reacting to the
/// package's own metadata one level up: the root-ancestor chain is the
/// pre-existing coverage; this pins it against regressions while the
/// discovered-file chains are added.
#[test]
fn watch_package_subdirectory_reacts_to_package_metadata() {
    let tmp = tempfile::tempdir().unwrap();
    let pkg = tmp.path().join("pkg");
    write_package_with_import(&pkg, "subdirwatch");
    std::fs::write(pkg.join("R/use.R"), "page <- tags\n").unwrap();
    let mut session = WatchSession::spawn(&pkg.join("R"));

    wait_for(&session.stderr, "watching 1 file(s)", "initial pass");
    assert!(
        !session.stdout.lock().unwrap().contains("RY010"),
        "the initial pass must resolve the imported name: {}",
        session.stdout.lock().unwrap()
    );

    std::fs::write(pkg.join("NAMESPACE"), "").unwrap();
    wait_for(&session.stdout, "RY010", "post-edit diagnostic");
    assert!(
        session.stdout.lock().unwrap().contains("`tags`"),
        "the re-check must flag the no-longer-imported name: {}",
        session.stdout.lock().unwrap()
    );
    session.assert_alive("after NAMESPACE edit");
}

/// Watching an explicit FILE keeps the file's package ancestors
/// tracked: a fresh one-shot of just that file resolves `tags` through
/// the nearest NAMESPACE, so the watch session must converge with it.
#[test]
fn watch_explicit_file_root_reacts_to_package_metadata() {
    let tmp = tempfile::tempdir().unwrap();
    let pkg = tmp.path().join("pkg");
    write_package_with_import(&pkg, "fileroot");
    let use_r = pkg.join("R/use.R");
    std::fs::write(&use_r, "page <- tags\n").unwrap();
    let mut session = WatchSession::spawn(&use_r);

    wait_for(&session.stderr, "watching 1 file(s)", "initial pass");
    assert!(
        !session.stdout.lock().unwrap().contains("RY010"),
        "the initial pass must resolve the imported name: {}",
        session.stdout.lock().unwrap()
    );

    std::fs::write(pkg.join("NAMESPACE"), "").unwrap();
    wait_for(&session.stdout, "RY010", "post-edit diagnostic");
    assert!(
        session.stdout.lock().unwrap().contains("`tags`"),
        "the re-check must flag the no-longer-imported name: {}",
        session.stdout.lock().unwrap()
    );
    session.assert_alive("after NAMESPACE edit");
}

/// Sibling packages track their own metadata: editing one package's
/// NAMESPACE re-checks and flags only that package's use of the import;
/// the sibling's own findings are untouched by the pass.
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

    // Dropping pkga's import must land exactly one new pass that flags
    // pkga's file while pkgb's finding is only re-printed once for that
    // one pass — the same screenful a fresh check of the tree prints,
    // with no extra pass and no extra sibling finding.
    let sibling_before = occurrences(&session.stdout, "`missing_in_b` is not bound");
    let passes_before = occurrences(&session.stderr, "checked 2 file(s)");
    std::fs::write(pkga.join("NAMESPACE"), "").unwrap();
    wait_for_more(
        &session.stderr,
        "checked 2 file(s)",
        passes_before,
        "post-edit pass",
    );
    wait_for(&session.stdout, "`tags`", "post-edit diagnostic");
    assert_eq!(
        occurrences(&session.stdout, "`missing_in_b` is not bound"),
        sibling_before + 1,
        "one pass must reprint pkgb's finding exactly once: {}",
        session.stdout.lock().unwrap()
    );
    session.assert_alive("after sibling metadata edit");
}

/// An initially empty workspace stays alive, picks up the first source
/// file, and then tracks the package metadata of the package that grows
/// around it: creating the DESCRIPTION re-checks (still flagged —
/// grouping does not bind the name), and creating the NAMESPACE binds
/// the import.
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

    // Growing a package around the file: the DESCRIPTION creation must
    // land a pass that still flags the name — once for that one pass,
    // the same screenful a fresh check prints.
    let tags_after_source = occurrences(&session.stdout, "`tags` is not bound");
    let passes_before = occurrences(&session.stderr, "checked 1 file(s)");
    std::fs::write(
        pkg.join("DESCRIPTION"),
        "Package: growaround\nVersion: 0.0.0.9000\n",
    )
    .unwrap();
    wait_for_more(
        &session.stderr,
        "checked 1 file(s)",
        passes_before,
        "post-DESCRIPTION pass",
    );
    // stdout and stderr are separate pipes drained by independent pump
    // threads, so the stderr summary can land before this pass's stdout
    // screenful is fully drained: synchronize on the stdout content
    // itself before comparing exact occurrence counts.
    wait_for_more(
        &session.stdout,
        "`tags` is not bound",
        tags_after_source,
        "post-DESCRIPTION diagnostic",
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

/// Moving a source file between package directories re-groups it: the
/// binding defined in one package resolves only while the use lives in
/// the same package. A rename preserves the file's mtime, so the pass
/// is driven by the file-set change and the re-derived metadata
/// dependencies, not by a touched timestamp.
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

/// A discovery-config change that admits previously excluded sources
/// must also grow the metadata dependency set to cover them: after the
/// excluded package's file enters the file set, editing that package's
/// NAMESPACE still converges with a fresh check.
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

/// Control: metadata of a tree discovery has no R files under — a
/// sibling directory that is neither a watched root nor an ancestor of
/// any discovered file — must NOT trigger a pass. Five polls elapse
/// (the fixed sleep is the harness's established negative-assertion
/// pattern; positive assertions above stay deadline-based).
#[test]
fn watch_ignores_unrelated_package_metadata() {
    let tmp = tempfile::tempdir().unwrap();
    let pkg = tmp.path().join("pkg");
    write_package_with_import(&pkg, "mainpkg");
    std::fs::write(pkg.join("R/use.R"), "page <- tags\n").unwrap();
    let mut session = WatchSession::spawn(tmp.path());

    wait_for(&session.stderr, "watching 1 file(s)", "initial pass");
    assert!(
        !session.stdout.lock().unwrap().contains("RY010"),
        "the initial pass must resolve the imported name: {}",
        session.stdout.lock().unwrap()
    );

    // Metadata of an R-less sibling tree: outside the dependency set.
    let unrelated = tmp.path().join("unrelated");
    std::fs::create_dir_all(&unrelated).unwrap();
    std::fs::write(
        unrelated.join("DESCRIPTION"),
        "Package: unrelated\nVersion: 0.0.0.9000\n",
    )
    .unwrap();
    std::fs::write(unrelated.join("NAMESPACE"), "importFrom(shiny,tags)\n").unwrap();

    let passes_before = occurrences(&session.stderr, "checked 1 file(s)");
    std::thread::sleep(Duration::from_millis(2500));
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
    session.assert_alive("after unrelated metadata creation");
}

/// Metadata ABOVE an absorbing package boundary must not trigger a
/// re-check either: the watched root is itself a package (DESCRIPTION
/// at the root), and a fresh one-shot groups every file by that NEAREST
/// existing DESCRIPTION — an ancestor walk that stops there, so a
/// DESCRIPTION/NAMESPACE pair created in a directory ABOVE the boundary
/// cannot change how any watched file resolves. The watch set stops at
/// the same boundary, so their creation stays silent. The final leg is
/// the control: the boundary's own NAMESPACE edit still re-checks.
#[test]
fn watch_ignores_metadata_above_absorbing_boundary() {
    let tmp = tempfile::tempdir().unwrap();
    let pkg = tmp.path().join("proj");
    write_package_with_import(&pkg, "boundary");
    std::fs::write(pkg.join("R/use.R"), "page <- tags\n").unwrap();
    let mut session = WatchSession::spawn(&pkg);

    // The root's own metadata resolves the import from the start.
    wait_for(&session.stderr, "watching 1 file(s)", "initial pass");
    assert!(
        !session.stdout.lock().unwrap().contains("RY010"),
        "the initial pass must resolve the imported name: {}",
        session.stdout.lock().unwrap()
    );

    // Unrelated package metadata one level ABOVE the boundary: outside
    // the dependency set, like the sibling-tree control above.
    std::fs::write(
        tmp.path().join("DESCRIPTION"),
        "Package: above\nVersion: 0.0.0.9000\n",
    )
    .unwrap();
    std::fs::write(tmp.path().join("NAMESPACE"), "importFrom(shiny,tags)\n").unwrap();

    let passes_before = occurrences(&session.stderr, "checked 1 file(s)");
    std::thread::sleep(Duration::from_millis(2500));
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

    // Control: the boundary's own metadata is inside the set.
    std::fs::write(pkg.join("NAMESPACE"), "").unwrap();
    wait_for(&session.stdout, "RY010", "post-boundary-edit diagnostic");
    assert!(
        session.stdout.lock().unwrap().contains("`tags`"),
        "the re-check must flag the no-longer-imported name: {}",
        session.stdout.lock().unwrap()
    );
    session.assert_alive("after boundary NAMESPACE edit");
}

/// A relative explicit-file root — `ry check pkg/R/use.R` run from the
/// parent directory — must track the same package ancestors as an
/// absolute one. The ancestor walk resolves its inputs against the
/// process working directory instead of terminating at the empty path
/// component `Path::new("pkg").parent()` yields, so the NAMESPACE one
/// level up still re-checks the file.
#[test]
fn watch_relative_root_reacts_to_package_metadata() {
    let tmp = tempfile::tempdir().unwrap();
    let pkg = tmp.path().join("pkg");
    write_package_with_import(&pkg, "relroot");
    let use_r = pkg.join("R/use.R");
    std::fs::write(&use_r, "page <- tags\n").unwrap();
    let mut session = WatchSession::spawn_at(Some(tmp.path()), std::path::Path::new("pkg/R/use.R"));

    wait_for(&session.stderr, "watching 1 file(s)", "initial pass");
    assert!(
        !session.stdout.lock().unwrap().contains("RY010"),
        "the initial pass must resolve the imported name: {}",
        session.stdout.lock().unwrap()
    );

    std::fs::write(pkg.join("NAMESPACE"), "").unwrap();
    wait_for(&session.stdout, "RY010", "post-edit diagnostic");
    assert!(
        session.stdout.lock().unwrap().contains("`tags`"),
        "the re-check must flag the no-longer-imported name: {}",
        session.stdout.lock().unwrap()
    );
    session.assert_alive("after NAMESPACE edit through a relative root");
}
