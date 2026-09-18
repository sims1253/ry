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
    fn spawn(dir: &std::path::Path) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_ry"))
            .arg("check")
            .arg("--watch")
            .arg("--color")
            .arg("never")
            .arg(dir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("failed to spawn ry check --watch");
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

    // Empty the baseline (accept nothing): the finding reappears although
    // no R file changed.
    std::fs::write(
        tmp.path().join("baseline.json"),
        "{\"version\": 1, \"entries\": []}\n",
    )
    .unwrap();
    wait_for(&session.stdout, "RY010", "post-reload diagnostic");
    assert!(
        session
            .stdout
            .lock()
            .unwrap()
            .contains("genuinely_missing_baseline_name"),
        "the re-check must report the un-accepted finding: {}",
        session.stdout.lock().unwrap()
    );

    // Deleting the config-level baseline settles to no baseline (like a
    // fresh run, which warns and continues without it) rather than
    // keeping the stale acceptances: the finding is already visible, so
    // the pin is that the session warns once and stays alive.
    std::fs::remove_file(tmp.path().join("baseline.json")).unwrap();
    wait_for(
        &session.stderr,
        "could not read baseline",
        "deletion warning",
    );
    session.assert_alive("after baseline deletion");

    // Restoring the seeded baseline re-accepts the finding: the next
    // pass goes quiet although no R file changed. There is deliberately
    // no recovery note (deletion was a settle, not a broken episode),
    // so the pin is the second zero-warning summary line — the first
    // came from the initial pass.
    let quiet_before = session
        .stderr
        .lock()
        .unwrap()
        .matches("0 warning(s)")
        .count();
    std::fs::write(tmp.path().join("baseline.json"), &seeded).unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if session
            .stderr
            .lock()
            .unwrap()
            .matches("0 warning(s)")
            .count()
            > quiet_before
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for the post-restore quiet pass; stderr: {}",
            session.stderr.lock().unwrap()
        );
        std::thread::sleep(Duration::from_millis(100));
    }
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
