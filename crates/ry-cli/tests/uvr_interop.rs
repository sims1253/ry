//! #588: an externally provisioned uvr library is an ordinary static R library.
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

fn missing_names(diagnostics: &[Value]) -> Vec<&str> {
    diagnostics
        .iter()
        .map(|item| {
            assert_eq!(item["code"], "RY010", "{item}");
            item["message"].as_str().unwrap()
        })
        .collect()
}

#[test]
fn cli_reads_uvr_metadata_without_attaching_or_scanning_the_library() {
    let fixture = UvrFixture::new();
    let installed = fixture.diagnostics(&fixture.library);
    assert_eq!(
        missing_names(&installed),
        ["variable `uvr_not_exported` is not bound in this scope"]
    );
    assert!(
        installed
            .iter()
            .all(|item| { item["path"].as_str() == Some(fixture.source().to_str().unwrap()) })
    );

    let missing = fixture.diagnostics(&fixture.project.join("missing library"));
    assert_eq!(
        missing_names(&missing),
        [
            "variable `uvr_export` is not bound in this scope",
            "variable `uvr_not_exported` is not bound in this scope"
        ]
    );
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
fn child_library_roots_preserve_order_and_missing_root_fallback() {
    let fixture = UvrFixture::new();
    let first = fixture.files.path("first library with spaces");
    let second = fixture.files.path("second library");
    install_export(&first, "uvr_first");
    install_export(&second, "uvr_second");
    fs::write(fixture.source(), "a <- uvr_first\nb <- uvr_second\n").unwrap();
    let first_then_second = std::env::join_paths([&first, &second]).unwrap();
    let second_then_first = std::env::join_paths([&second, &first]).unwrap();
    for (roots, missing) in [
        (first_then_second, "uvr_second"),
        (second_then_first, "uvr_first"),
    ] {
        let output = fixture
            .command()
            .args(["check", "--output-format", "json"])
            .arg(&fixture.project)
            .env("R_LIBS", roots)
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        let diagnostics: Vec<Value> = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert!(
            diagnostics[0]["message"]
                .as_str()
                .unwrap()
                .contains(missing),
            "{diagnostics:?}"
        );
    }
    let fallback = fixture
        .command()
        .args(["check", "--output-format", "json"])
        .arg(&fixture.project)
        .env("R_LIBS", fixture.project.join("missing library"))
        .env("R_LIBS_USER", &second)
        .output()
        .unwrap();
    assert!(fallback.status.success(), "{fallback:?}");
    let diagnostics: Vec<Value> = serde_json::from_slice(&fallback.stdout).unwrap();
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert!(
        diagnostics[0]["message"]
            .as_str()
            .unwrap()
            .contains("uvr_first")
    );

    let site_fallback = fixture
        .command()
        .args(["check", "--output-format", "json"])
        .arg(&fixture.project)
        .env("R_LIBS", fixture.project.join("missing library"))
        .env("R_LIBS_USER", fixture.project.join("missing user library"))
        .env("R_LIBS_SITE", &first)
        .output()
        .unwrap();
    assert!(site_fallback.status.success(), "{site_fallback:?}");
    let diagnostics: Vec<Value> = serde_json::from_slice(&site_fallback.stdout).unwrap();
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert!(
        diagnostics[0]["message"]
            .as_str()
            .unwrap()
            .contains("uvr_second")
    );
}

#[test]
fn nearest_renv_library_precedes_an_explicit_uvr_root() {
    let fixture = UvrFixture::new();
    let renv = fixture.project.join("renv/library");
    install_export(&renv, "renv_export");
    fs::write(fixture.source(), "a <- renv_export\nb <- uvr_export\n").unwrap();
    let diagnostics = fixture.diagnostics(&fixture.library);
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert!(
        diagnostics[0]["message"]
            .as_str()
            .unwrap()
            .contains("uvr_export")
    );

    fs::remove_dir_all(&renv).unwrap();
    let diagnostics = fixture.diagnostics(&fixture.library);
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert!(
        diagnostics[0]["message"]
            .as_str()
            .unwrap()
            .contains("renv_export")
    );
}

#[test]
fn nested_library_versions_follow_the_inferred_r_minor() {
    let fixture = UvrFixture::new();
    let versions = fixture.files.path("versioned libraries with spaces");
    install_export(&versions.join("4.3"), "uvr_old");
    install_export(&versions.join("4.4"), "uvr_new");
    fs::write(fixture.source(), "a <- uvr_old\nb <- uvr_new\n").unwrap();
    for (version, missing) in [("4.3", "uvr_new"), ("4.4", "uvr_old")] {
        let r_home = fixture.files.path(format!("r-home-{version}"));
        fs::create_dir_all(r_home.join("library/base")).unwrap();
        fs::write(r_home.join("library/base/NAMESPACE"), "export(baseenv)\n").unwrap();
        fs::write(
            r_home.join("library/base/DESCRIPTION"),
            format!("Package: base\nVersion: {version}.1\n"),
        )
        .unwrap();
        let output = fixture
            .command()
            .args(["check", "--output-format", "json"])
            .arg(&fixture.project)
            .env("R_LIBS", versions.join("%v"))
            .env("R_HOME", r_home)
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        let diagnostics: Vec<Value> = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(diagnostics.len(), 1, "{version}: {diagnostics:?}");
        assert!(
            diagnostics[0]["message"]
                .as_str()
                .unwrap()
                .contains(missing),
            "{version}: {diagnostics:?}"
        );
    }
    install_export(&versions, "uvr_direct");
    fs::write(
        fixture.source(),
        "a <- uvr_direct\nb <- uvr_old\nc <- uvr_new\n",
    )
    .unwrap();
    let output = fixture
        .command()
        .args(["check", "--output-format", "json"])
        .arg(&fixture.project)
        .env("R_LIBS", versions.join("%v"))
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let diagnostics: Vec<Value> = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(missing_names(&diagnostics).len(), 2, "{diagnostics:?}");
    assert!(diagnostics.iter().any(|item| {
        item["message"]
            .as_str()
            .is_some_and(|message| message.contains("uvr_old"))
    }));
    assert!(diagnostics.iter().any(|item| {
        item["message"]
            .as_str()
            .is_some_and(|message| message.contains("uvr_new"))
    }));
}

#[test]
fn lsp_reads_the_environment_at_launch_like_the_cli() {
    let fixture = UvrFixture::new();
    let installed = fixture.lsp_diagnostics(&fixture.library);
    let missing = fixture.lsp_diagnostics(&fixture.project.join("missing library"));
    assert_eq!(installed.len(), 1, "{installed:?}");
    assert_eq!(missing.len(), 2, "{missing:?}");
    assert!(
        installed[0]["message"]
            .as_str()
            .unwrap()
            .contains("uvr_not_exported")
    );
    assert!(missing.iter().any(|item| {
        item["message"]
            .as_str()
            .is_some_and(|message| message.contains("uvr_export"))
    }));
    assert!(missing.iter().any(|item| {
        item["message"]
            .as_str()
            .is_some_and(|message| message.contains("uvr_not_exported"))
    }));
}
