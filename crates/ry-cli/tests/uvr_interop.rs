//! An externally provisioned uvr library is an ordinary static R library.
//! Every environment change is confined to a child process; these tests never
//! run R, activate uvr, or mutate the test process environment.

use ry_testkit::{FixtureProject, JsonRpcProcess, file_uri};
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

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
        install_export(&library, "uvr_export");
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
        self.project.join("R/use.R")
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

    fn diagnostics(&self, library: &Path) -> Vec<Value> {
        let output = self
            .command()
            .args(["check", "--output-format", "json"])
            .arg(&self.project)
            .env("R_LIBS", library)
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn facts(&self, library: &Path) -> Value {
        let output = self
            .command()
            .args(["dump-facts", "--format", "json"])
            .arg(&self.project)
            .env("R_LIBS", library)
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn lsp_diagnostics(&self, library: &Path) -> Vec<Value> {
        let mut command = self.command();
        command.arg("server").env("R_LIBS", library);
        let mut client = JsonRpcProcess::spawn(&mut command).unwrap();
        let root_uri = file_uri(&self.project).unwrap();
        let source_uri = file_uri(&self.source()).unwrap();
        let initialize = client
            .request(
                "initialize",
                json!({
                    "processId": null,
                    "rootUri": root_uri,
                    "capabilities": {},
                    "workspaceFolders": [{"uri": root_uri, "name": "uvr fixture"}]
                }),
            )
            .unwrap();
        client
            .receive_until(|message| message.get("id") == Some(&json!(initialize)), 32)
            .unwrap();
        client.notify("initialized", json!({})).unwrap();
        client
            .notify(
                "textDocument/didOpen",
                json!({
                    "textDocument": {
                        "uri": source_uri,
                        "languageId": "r",
                        "version": 1,
                        "text": SOURCE
                    }
                }),
            )
            .unwrap();
        let publication = client
            .receive_until(
                |message| {
                    message.get("method") == Some(&json!("textDocument/publishDiagnostics"))
                        && message.pointer("/params/uri") == Some(&json!(source_uri))
                },
                64,
            )
            .unwrap();
        let diagnostics = publication["params"]["diagnostics"]
            .as_array()
            .unwrap()
            .clone();
        let shutdown = client.request("shutdown", Value::Null).unwrap();
        client
            .receive_until(|message| message.get("id") == Some(&json!(shutdown)), 16)
            .unwrap();
        client.notify("exit", Value::Null).unwrap();
        diagnostics
    }
}

fn install_export(library: &Path, export: &str) {
    let package = library.join(PACKAGE);
    fs::create_dir_all(&package).unwrap();
    fs::write(package.join("NAMESPACE"), format!("export({export})\n")).unwrap();
}

/// The names RY010 reports unbound, in output order.
fn missing_names(diagnostics: &[Value]) -> Vec<&str> {
    diagnostics
        .iter()
        .map(|item| {
            assert_eq!(item["code"], "RY010", "{item}");
            item["message"].as_str().unwrap().split('`').nth(1).unwrap()
        })
        .collect()
}

#[test]
fn cli_reads_uvr_metadata_without_attaching_or_scanning_the_library() {
    let fixture = UvrFixture::new();
    let installed = fixture.diagnostics(&fixture.library);
    assert_eq!(missing_names(&installed), ["uvr_not_exported"]);
    assert!(
        installed
            .iter()
            .all(|item| { item["path"].as_str() == Some(fixture.source().to_str().unwrap()) })
    );

    let missing = fixture.diagnostics(&fixture.project.join("missing library"));
    assert_eq!(missing_names(&missing), ["uvr_export", "uvr_not_exported"]);
    let installed_facts = fixture.facts(&fixture.library);
    let missing_facts = fixture.facts(&fixture.project.join("missing library"));
    assert_eq!(
        installed_facts["files"][0]["source_hash"],
        missing_facts["files"][0]["source_hash"]
    );
    assert_ne!(
        installed_facts["files"][0]["context_id"], missing_facts["files"][0]["context_id"],
        "installed NAMESPACE inventory must change the consumed context"
    );
    assert!(
        installed_facts["files"]
            .as_array()
            .unwrap()
            .iter()
            .all(|file| { !file["path"].as_str().unwrap().contains(".uvr/library") })
    );

    // uvr.toml and uvr.lock alone do not attach the installed package.
    fs::write(fixture.project.join("NAMESPACE"), "").unwrap();
    let unattached = fixture.diagnostics(&fixture.library);
    assert_eq!(missing_names(&unattached), missing_names(&missing));
}

#[test]
fn lsp_reads_the_environment_at_launch_like_the_cli() {
    let fixture = UvrFixture::new();
    let installed = fixture.lsp_diagnostics(&fixture.library);
    let missing = fixture.lsp_diagnostics(&fixture.project.join("missing library"));
    assert_eq!(missing_names(&installed), ["uvr_not_exported"]);
    let mut missing = missing_names(&missing);
    missing.sort_unstable();
    assert_eq!(missing, ["uvr_export", "uvr_not_exported"]);
}
