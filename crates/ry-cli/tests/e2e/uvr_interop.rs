//! An externally provisioned uvr library is an ordinary static R library.
//! Every environment change is confined to a child process; these tests never
//! run R, activate uvr, or mutate the test process environment.

use ry_testkit::{FixtureProject, JsonRpcProcess, file_uri};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::support::{install_namespace, unbound_names};

const PACKAGE: &str = "ryuvrfixture";
const SOURCE: &str = "from_library <- uvr_export\nnot_exported <- uvr_not_exported\n";

struct UvrFixture {
    files: FixtureProject,
    project: PathBuf,
    library: PathBuf,
}

impl UvrFixture {
    fn new() -> Self {
        let files = FixtureProject::empty().unwrap();
        let project = files.path("project with spaces");
        files
            .write_file(
                "project with spaces/DESCRIPTION",
                "Package: uvrconsumer\nVersion: 0.1.0\nImports: ryuvrfixture\n",
            )
            .unwrap();
        files
            .write_file("project with spaces/NAMESPACE", "import(ryuvrfixture)\n")
            .unwrap();
        files
            .write_file("project with spaces/R/use.R", SOURCE)
            .unwrap();
        files
            .write_file(
                "project with spaces/uvr.toml",
                "[dependencies]\nryuvrfixture = \"*\"\n",
            )
            .unwrap();
        files
            .write_file("project with spaces/uvr.lock", "locked_only_name\n")
            .unwrap();
        let library = project.join(".uvr/library");
        install_namespace(&library, PACKAGE, "uvr_export");
        fs::create_dir_all(library.join(PACKAGE).join("R")).unwrap();
        fs::write(
            library.join(PACKAGE).join("R/not_project_source.R"),
            "should_not_be_checked <- genuinely_missing_name\n",
        )
        .unwrap();
        Self {
            files,
            project,
            library,
        }
    }

    fn source(&self) -> PathBuf {
        // Joined per component: Windows keeps a literal `/` in `join("R/use.R")`,
        // which would not match the path ry reports.
        self.project.join("R").join("use.R")
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_ry"));
        command.current_dir(&self.project);
        // Only the child's environment is isolated. A unique fixture package
        // prevents system-library fallbacks from influencing the assertion.
        for key in [
            "R_LIBS",
            "R_LIBS_USER",
            "R_LIBS_SITE",
            "R_HOME",
            "HOME",
            "USERPROFILE",
            "LOCALAPPDATA",
            "APPDATA",
            "RY_NO_INSTALLED_LIBRARIES",
        ] {
            command.env_remove(key);
        }
        command.env("HOME", self.files.path("empty-home"));
        command.env("USERPROFILE", self.files.path("empty-profile"));
        command
    }

    fn diagnostics(&self, library: Option<&Path>) -> Vec<Value> {
        let mut cmd = self.command();
        cmd.args(["check", "--output-format", "json"])
            .arg(&self.project);
        if let Some(library) = library {
            cmd.env("R_LIBS", library);
        }
        let output = cmd.output().unwrap();
        assert!(output.status.success(), "{output:?}");
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn facts(&self, library: Option<&Path>) -> Value {
        let mut cmd = self.command();
        cmd.args(["dump-facts", "--format", "json"])
            .arg(&self.project);
        if let Some(library) = library {
            cmd.env("R_LIBS", library);
        }
        let output = cmd.output().unwrap();
        assert!(output.status.success(), "{output:?}");
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn lsp_diagnostics(&self, library: Option<&Path>) -> Vec<Value> {
        let mut command = self.command();
        command.arg("server");
        if let Some(library) = library {
            command.env("R_LIBS", library);
        }
        JsonRpcProcess::spawn(&mut command)
            .unwrap()
            .open_and_publish(&self.project, &file_uri(&self.source()).unwrap(), SOURCE)
            .unwrap()
    }
}

#[test]
fn cli_reads_uvr_metadata_without_attaching_or_scanning_the_library() {
    let fixture = UvrFixture::new();
    // Auto-discovers `.uvr/library` when `uvr.toml`/`uvr.lock` is present (#812).
    let auto_discovered = fixture.diagnostics(None);
    assert_eq!(unbound_names(&auto_discovered), ["uvr_not_exported"]);
    assert!(
        auto_discovered
            .iter()
            .all(|item| { item["path"].as_str() == Some(fixture.source().to_str().unwrap()) })
    );

    // Explicit `R_LIBS` continues to work too.
    let installed = fixture.diagnostics(Some(&fixture.library));
    assert_eq!(unbound_names(&installed), ["uvr_not_exported"]);

    let installed_facts = fixture.facts(None);
    assert!(
        installed_facts["files"]
            .as_array()
            .unwrap()
            .iter()
            .all(|file| { !file["path"].as_str().unwrap().contains(".uvr/library") })
    );

    // Removing `.uvr/library` drops the discovered package and changes context_id.
    fs::remove_dir_all(&fixture.library).unwrap();
    let missing = fixture.diagnostics(None);
    assert_eq!(unbound_names(&missing), ["uvr_export", "uvr_not_exported"]);
    let missing_facts = fixture.facts(None);
    assert_eq!(
        installed_facts["files"][0]["source_hash"],
        missing_facts["files"][0]["source_hash"]
    );
    assert_ne!(
        installed_facts["files"][0]["context_id"], missing_facts["files"][0]["context_id"],
        "installed NAMESPACE inventory must change the consumed context"
    );

    // uvr.toml and uvr.lock alone do not attach the installed package.
    install_namespace(&fixture.library, PACKAGE, "uvr_export");
    fs::write(fixture.project.join("NAMESPACE"), "").unwrap();
    let unattached = fixture.diagnostics(None);
    assert_eq!(unbound_names(&unattached), unbound_names(&missing));
}

#[test]
fn lsp_reads_the_environment_at_launch_like_the_cli() {
    let fixture = UvrFixture::new();
    // LSP auto-discovers `.uvr/library` without R_LIBS (#812).
    let auto_discovered = fixture.lsp_diagnostics(None);
    assert_eq!(unbound_names(&auto_discovered), ["uvr_not_exported"]);

    // Without `.uvr/library`, both names are unbound.
    fs::remove_dir_all(&fixture.library).unwrap();
    let mut missing = unbound_names(&fixture.lsp_diagnostics(None));
    missing.sort_unstable();
    assert_eq!(missing, ["uvr_export", "uvr_not_exported"]);
}
