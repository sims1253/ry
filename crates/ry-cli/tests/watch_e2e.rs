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
    std::fs::write(tmp.path().join("main.R"), "result <- genuinely_missing_name\n").unwrap();
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
