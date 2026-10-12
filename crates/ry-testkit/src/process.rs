use std::ffi::OsStr;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Output, Stdio};

use serde_json::Value;

use crate::FixtureProject;

/// A [`CliProcess`] for the calling test crate's `ry` binary.
#[macro_export]
macro_rules! ry {
    () => {
        $crate::CliProcess::new(env!("CARGO_BIN_EXE_ry"))
    };
}

/// One real CLI subprocess, configured like a [`Command`] and run once.
/// Stdin is empty unless [`CliProcess::stdin`] supplies bytes.
pub struct CliProcess {
    command: Command,
    stdin: Option<Vec<u8>>,
}

impl CliProcess {
    pub fn new(binary: impl Into<PathBuf>) -> Self {
        Self {
            command: Command::new(binary.into()),
            stdin: None,
        }
    }

    pub fn cwd(mut self, dir: impl AsRef<Path>) -> Self {
        self.command.current_dir(dir);
        self
    }

    pub fn arg(mut self, arg: impl AsRef<OsStr>) -> Self {
        self.command.arg(arg);
        self
    }

    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.command.args(args);
        self
    }

    pub fn env(mut self, key: impl AsRef<OsStr>, value: impl AsRef<OsStr>) -> Self {
        self.command.env(key, value);
        self
    }

    pub fn env_remove(mut self, key: impl AsRef<OsStr>) -> Self {
        self.command.env_remove(key);
        self
    }

    pub fn stdin(mut self, input: impl Into<Vec<u8>>) -> Self {
        self.stdin = Some(input.into());
        self
    }

    /// Run to completion. Spawn failures panic: they are harness errors.
    pub fn run(mut self) -> CliOutput {
        let output = match self.stdin.take() {
            None => self.command.stdin(Stdio::null()).output(),
            Some(input) => self.output_with_stdin(input),
        };
        CliOutput(output.expect("spawn ry"))
    }

    fn output_with_stdin(mut self, input: Vec<u8>) -> io::Result<Output> {
        let mut child = self
            .command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let mut stdin = child.stdin.take().expect("piped stdin");
        let writer = std::thread::spawn(move || stdin.write_all(&input));
        let output = child.wait_with_output()?;
        // A process that rejects its arguments may exit before reading.
        match writer.join().expect("stdin writer") {
            Err(error) if error.kind() != io::ErrorKind::BrokenPipe => Err(error),
            _ => Ok(output),
        }
    }

    /// `ry check <arguments> <target>` from the fixture root.
    pub fn check<I, S>(
        self,
        fixture: &FixtureProject,
        target: impl AsRef<Path>,
        arguments: I,
    ) -> io::Result<Output>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let mut command = self.command;
        command.current_dir(fixture.root()).arg("check");
        command.args(arguments);
        command.arg(target.as_ref());
        command.output()
    }
}

/// A finished CLI run with the accessors e2e assertions need.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliOutput(pub Output);

impl CliOutput {
    pub fn status(&self) -> ExitStatus {
        self.0.status
    }

    pub fn code(&self) -> Option<i32> {
        self.0.status.code()
    }

    pub fn success(&self) -> bool {
        self.0.status.success()
    }

    pub fn stdout(&self) -> String {
        String::from_utf8_lossy(&self.0.stdout).into_owned()
    }

    pub fn stderr(&self) -> String {
        String::from_utf8_lossy(&self.0.stderr).into_owned()
    }

    /// Stdout as JSON; panics with both streams when it is not.
    pub fn json(&self) -> Value {
        serde_json::from_slice(&self.0.stdout)
            .unwrap_or_else(|error| panic!("stdout is not JSON ({error}): {self:?}"))
    }

    /// Rule codes of a `--output-format json` diagnostic array, in order.
    pub fn codes(&self) -> Vec<String> {
        self.diagnostics()
            .iter()
            .map(|diagnostic| diagnostic["code"].as_str().unwrap().to_owned())
            .collect()
    }

    /// `[path, code, severity]` of each JSON diagnostic, in order.
    pub fn findings(&self) -> Vec<[String; 3]> {
        self.diagnostics()
            .iter()
            .map(|diagnostic| {
                ["path", "code", "severity"].map(|key| diagnostic[key].as_str().unwrap().to_owned())
            })
            .collect()
    }

    fn diagnostics(&self) -> Vec<Value> {
        match self.json() {
            Value::Array(diagnostics) => diagnostics,
            other => panic!("expected a diagnostic array: {other}"),
        }
    }
}
