//! Real-process edit replay. The default one-sample run is a correctness gate;
//! the Performance workflow requests enough warm samples for latency metrics.

mod harness;

use harness::{Published, published_from_cli_value, published_from_lsp, ry_binary};
use ry_testkit::{CliProcess, DriverError, FixtureProject, LspSession, file_uri};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};
use tokio::process::{Child, ChildStdin, ChildStdout};

type Session = LspSession<ChildStdout, ChildStdin>;
const DRAIN_IDLE: Duration = Duration::from_millis(60);

#[derive(Deserialize)]
struct Workload {
    schema_version: u32,
    files: BTreeMap<String, String>,
    scenarios: Vec<Scenario>,
}

#[derive(Deserialize)]
struct Scenario {
    id: String,
    edit: String,
    reset: String,
    #[serde(rename = "final")]
    final_text: String,
    completion: String,
}

struct Replay {
    live: FixtureProject,
    fresh: FixtureProject,
    session: Session,
    child: Child,
    binary: PathBuf,
    sources: BTreeMap<String, String>,
    versions: BTreeMap<String, i32>,
    uri_to_path: BTreeMap<String, String>,
    published: BTreeMap<String, Vec<Published>>,
}

impl Replay {
    async fn start(binary: &Path, workload: &Workload) -> Result<(Self, u64), DriverError> {
        let live = FixtureProject::empty()?;
        let fresh = FixtureProject::empty()?;
        let mut uri_to_path = BTreeMap::new();
        for (path, source) in &workload.files {
            live.write_file(path, source)?;
            fresh.write_file(path, source)?;
            uri_to_path.insert(file_uri(&live.path(path))?, path.clone());
        }

        let mut command = tokio::process::Command::new(binary);
        let started = Instant::now();
        let mut child = command
            .arg("server")
            .current_dir(live.root())
            .kill_on_drop(true)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::inherit())
            .spawn()?;
        let stdout = child.stdout.take().ok_or("server stdout is not piped")?;
        let stdin = child.stdin.take().ok_or("server stdin is not piped")?;
        let mut session = LspSession::new(stdout, stdin);
        session.initialize(live.root()).await?;
        let mark = session.publication_mark();
        for (path, source) in &workload.files {
            session
                .open(&file_uri(&live.path(path))?, 1, source)
                .await?;
        }
        let local_uri = file_uri(&live.path("R/local.R"))?;
        let checkpoint = session
            .quiesce_diagnostics_timed(&local_uri, mark, DRAIN_IDLE)
            .await?;
        let startup_ns = nanos(checkpoint.first_received_at.duration_since(started));
        assert_eq!(checkpoint.first.pointer("/params/version"), Some(&json!(1)));

        let mut replay = Self {
            live,
            fresh,
            session,
            child,
            binary: binary.to_path_buf(),
            sources: workload.files.clone(),
            versions: workload.files.keys().map(|p| (p.clone(), 1)).collect(),
            uri_to_path,
            published: BTreeMap::new(),
        };
        replay.apply_publications(checkpoint.publications);
        // Initial readiness is a real publication plus fresh analysis, not a
        // fixed wait after initialize/didOpen.
        replay.assert_fresh()?;
        Ok((replay, startup_ns))
    }

    fn cli_diagnostics(&self) -> Result<Vec<Published>, DriverError> {
        let output = CliProcess::new(&self.binary).check(
            &self.fresh,
            self.fresh.root(),
            ["--output-format", "json"],
        )?;
        assert!(
            matches!(output.status.code(), Some(0 | 1)),
            "fresh CLI failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let values: Vec<Value> = serde_json::from_slice(&output.stdout)?;
        let mut findings: Vec<_> = values
            .iter()
            .map(|value| published_from_cli_value(value, self.fresh.root()))
            .collect();
        findings.sort();
        Ok(findings)
    }

    fn apply_publications(&mut self, publications: BTreeMap<String, Vec<Value>>) {
        for (uri, diagnostics) in publications {
            let relative = self
                .uri_to_path
                .get(&uri)
                .unwrap_or_else(|| panic!("publication outside replay scope: {uri}"));
            let message = json!({"params": {"diagnostics": diagnostics}});
            let normalized =
                published_from_lsp(&message, &self.fresh.path(relative), self.fresh.root());
            self.published.insert(relative.clone(), normalized);
        }
    }

    fn assert_fresh(&self) -> Result<(), DriverError> {
        let expected = self.cli_diagnostics()?;
        let mut actual: Vec<_> = self.published.values().flatten().cloned().collect();
        actual.sort();
        assert_eq!(actual, expected, "live publication differs from fresh CLI");
        Ok(())
    }

    async fn edit(
        &mut self,
        path: &str,
        text: &str,
        completion: &str,
    ) -> Result<Value, DriverError> {
        self.fresh.write_file(path, text)?;
        self.sources.insert(path.to_string(), text.to_string());
        let expected = self.cli_diagnostics()?;
        let expected_target: Vec<_> = expected
            .iter()
            .filter(|item| item.path == completion)
            .cloned()
            .collect();

        let edited_version = {
            let version = self.versions.get_mut(path).ok_or("edited path is absent")?;
            *version += 1;
            *version
        };
        let edited_uri = file_uri(&self.live.path(path))?;
        let target_uri = file_uri(&self.live.path(completion))?;
        let target_version = *self
            .versions
            .get(completion)
            .ok_or("completion path is absent")?;
        let mark = self.session.publication_mark();
        let started = Instant::now();
        self.session
            .change(&edited_uri, edited_version, json!([{"text": text}]))
            .await?;
        let checkpoint = self
            .session
            .quiesce_diagnostics_timed(&target_uri, mark, DRAIN_IDLE)
            .await?;
        assert_eq!(
            checkpoint.first.pointer("/params/version"),
            Some(&json!(target_version)),
            "completion lacks the analyzed open-document version"
        );
        let first = published_from_lsp(
            &checkpoint.first,
            &self.fresh.path(completion),
            self.fresh.root(),
        );
        assert_eq!(first, expected_target, "first target publication is stale");
        self.apply_publications(checkpoint.publications);
        self.assert_fresh()?;
        Ok(json!({
            "duration_ns": nanos(checkpoint.first_received_at.duration_since(started)),
            "snapshot_sha256": hex_hash(&serde_json::to_vec(&self.sources)?),
            "edited_version": edited_version,
            "completion_version": target_version,
            "completion": completion,
        }))
    }

    async fn reset_all(&mut self, workload: &Workload) -> Result<(), DriverError> {
        for (path, initial) in &workload.files {
            if self.sources.get(path) != Some(initial) {
                self.edit(path, initial, path).await?;
            }
        }
        self.assert_fresh()
    }

    async fn finish(mut self) -> Result<(), DriverError> {
        self.session.shutdown().await?;
        drop(self.session);
        let status = tokio::time::timeout(Duration::from_secs(5), self.child.wait()).await??;
        assert!(status.success(), "server did not exit cleanly: {status}");
        Ok(())
    }
}

fn nanos(duration: Duration) -> u64 {
    u64::try_from(duration.as_nanos()).expect("measurement fits in u64 nanoseconds")
}

fn hex_hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn required_count(name: &str, default: usize) -> usize {
    env::var(name).map_or(default, |value| {
        let count = value
            .parse::<usize>()
            .expect("sample count must be an integer");
        assert!((1..=100).contains(&count), "sample count must be 1..=100");
        count
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn server_edit_replay_matches_fresh_cli() -> Result<(), DriverError> {
    let workload_path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata/server-replay/v1.json");
    let workload_bytes = fs::read(&workload_path)?;
    let workload: Workload = serde_json::from_slice(&workload_bytes)?;
    assert_eq!(workload.schema_version, 1);
    assert_eq!(workload.scenarios.len(), 3);

    let binary = env::var_os("RY_REPLAY_BINARY")
        .map(PathBuf::from)
        .unwrap_or_else(ry_binary)
        .canonicalize()?;
    let output = env::var_os("RY_REPLAY_OUTPUT").map(PathBuf::from);
    let samples = required_count("RY_REPLAY_SAMPLES", 1);
    let startups = required_count("RY_REPLAY_STARTUPS", 1);
    if output.is_some() {
        assert!(
            samples >= 30,
            "reported tail latency needs at least 30 warm samples"
        );
        assert!(
            startups >= 5,
            "reported startup median needs five fresh processes"
        );
    }

    let mut startup_ns = Vec::new();
    for _ in 0..startups {
        let (replay, duration) = Replay::start(&binary, &workload).await?;
        startup_ns.push(duration);
        replay.finish().await?;
    }

    let (mut replay, _) = Replay::start(&binary, &workload).await?;
    let mut warm = BTreeMap::new();
    for scenario in &workload.scenarios {
        let mut observations = Vec::new();
        for sample in 0..samples {
            replay.reset_all(&workload).await?;
            let reset = scenario.reset.replace("{sample}", &sample.to_string());
            if replay.sources.get(&scenario.edit) != Some(&reset) {
                replay
                    .edit(&scenario.edit, &reset, &scenario.completion)
                    .await?;
            }
            let final_text = scenario.final_text.replace("{sample}", &sample.to_string());
            observations.push(
                replay
                    .edit(&scenario.edit, &final_text, &scenario.completion)
                    .await?,
            );
        }
        warm.insert(scenario.id.clone(), observations);
    }
    replay.finish().await?;

    if let Some(path) = output {
        let version = Command::new(&binary).arg("--version").output()?;
        assert!(version.status.success());
        let rustc = Command::new("rustc").arg("-vV").output()?;
        assert!(rustc.status.success());
        let report = json!({
            "schema_version": 1,
            "workload_sha256": hex_hash(&workload_bytes),
            "binary_sha256": hex_hash(&fs::read(&binary)?),
            "binary_version": String::from_utf8(version.stdout)?.trim(),
            "rustc": String::from_utf8(rustc.stdout)?.trim(),
            "rayon_threads": env::var("RAYON_NUM_THREADS").unwrap_or_else(|_| "default".into()),
            "sample_counts": {"startup": startups, "warm_per_scenario": samples},
            "startup_ns": startup_ns,
            "warm": warm,
            "correctness": "passed",
        });
        fs::write(path, serde_json::to_vec_pretty(&report)?)?;
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn analysis_clear_carries_version_but_close_clear_does_not() -> Result<(), DriverError> {
    let fixture = FixtureProject::empty()?;
    let path = fixture.write_file("R/a.R", "value <- missing_name\n")?;
    let uri = file_uri(&path)?;
    let (mut session, server) = harness::spawn_session(&[fixture.root()], json!({}), None).await;

    let mark = session.publication_mark();
    session.open(&uri, 1, "value <- missing_name\n").await?;
    let opened = session
        .quiesce_diagnostics_timed(&uri, mark, DRAIN_IDLE)
        .await?;
    assert_eq!(opened.first.pointer("/params/version"), Some(&json!(1)));
    assert!(
        opened.first["params"]["diagnostics"]
            .as_array()
            .is_some_and(|diagnostics| !diagnostics.is_empty())
    );

    let mark = session.publication_mark();
    session
        .change(&uri, 2, json!([{"text": "value <- 1L\n"}]))
        .await?;
    let cleared = session
        .quiesce_diagnostics_timed(&uri, mark, DRAIN_IDLE)
        .await?;
    assert_eq!(cleared.first.pointer("/params/version"), Some(&json!(2)));
    assert_eq!(cleared.first["params"]["diagnostics"], json!([]));

    let mark = session.publication_mark();
    session
        .notify(
            "textDocument/didClose",
            json!({"textDocument": {"uri": uri}}),
        )
        .await?;
    let closed = session.published_diagnostics_after(&uri, mark).await?;
    assert!(closed.pointer("/params/version").is_none_or(Value::is_null));
    assert_eq!(closed["params"]["diagnostics"], json!([]));
    harness::join_session(session, server).await;
    Ok(())
}
