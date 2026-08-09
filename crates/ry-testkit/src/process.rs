use std::ffi::OsStr;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use crate::FixtureProject;

/// Real CLI subprocess mechanics. Output interpretation belongs to a CLI test adapter.
pub struct CliProcess {
    binary: PathBuf,
}

impl CliProcess {
    pub fn new(binary: impl Into<PathBuf>) -> Self {
        Self {
            binary: binary.into(),
        }
    }

    pub fn check<I, S>(
        &self,
        fixture: &FixtureProject,
        target: impl AsRef<Path>,
        arguments: I,
    ) -> io::Result<Output>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let mut command = Command::new(&self.binary);
        command.current_dir(fixture.root()).arg("check");
        command.args(arguments);
        command.arg(target.as_ref());
        command.output()
    }
}
