//! Real CLI and LSP entry points for package-local box modules.
use ry_testkit::{FixtureProject, JsonRpcProcess, file_uri};
use serde_json::{Value, json};
use std::process::Command;

const SOURCE: &str = "box::use(./modules/hello[foo, missing])\nbox::use(dplyr[filter])\nd <- data.frame(mpg = c(21, 22.8))\nchosen <- filter(d, mpg > 21)\nbad <- foo() + 1L\n";
const MODULE: &str = "foo <- function() 'hello'\nbox::export(foo)\n";

fn fixture() -> FixtureProject {
    let files = FixtureProject::empty().unwrap();
    files
        .write_file(
            "DESCRIPTION",
            "Package: boxconsumer\nVersion: 0.1.0\nImports: box, dplyr\n",
        )
        .unwrap();
    files.write_file("NAMESPACE", "").unwrap();
    files.write_file("R/use.R", SOURCE).unwrap();
    files.write_file("R/modules/hello.r", MODULE).unwrap();
    files
}

fn codes(diagnostics: &[Value]) -> Vec<String> {
    diagnostics
        .iter()
        .map(|diagnostic| diagnostic["code"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn package_local_module_and_dplyr_import_reach_cli_pipeline() {
    let files = fixture();
    let output = Command::new(env!("CARGO_BIN_EXE_ry"))
        .args(["check", "--output-format", "json"])
        .arg(files.root())
        .env("RY_NO_INSTALLED_LIBRARIES", "1")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let diagnostics: Vec<Value> = serde_json::from_slice(&output.stdout).unwrap();
    let codes = codes(&diagnostics);
    assert!(codes.contains(&"RY118".to_string()), "{diagnostics:#?}");
    assert!(codes.contains(&"RY040".to_string()), "{diagnostics:#?}");
    assert!(!codes.contains(&"RY010".to_string()), "{diagnostics:#?}");
}

#[test]
fn installed_namespace_gates_package_stub_import_without_loading_r() {
    let files = FixtureProject::empty().unwrap();
    files
        .write_file(
            "run.R",
            "box::use(dplyr[filter])\nd <- data.frame(mpg = 1L)\nfilter(d, mpg > 0)\n",
        )
        .unwrap();
    files
        .write_file("lib/dplyr/DESCRIPTION", "Package: dplyr\nVersion: 1.2.1\n")
        .unwrap();
    let namespace = files.path("lib/dplyr/NAMESPACE");
    let run = || {
        let output = Command::new(env!("CARGO_BIN_EXE_ry"))
            .args(["check", "--output-format", "json", "--exit-zero"])
            .arg(files.path("run.R"))
            .env_remove("RY_NO_INSTALLED_LIBRARIES")
            .env("R_LIBS", files.path("lib"))
            .env("R_LIBS_USER", files.path("missing-user-lib"))
            .env("R_LIBS_SITE", files.path("missing-site-lib"))
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(0), "{output:?}");
        let diagnostics: Vec<Value> = serde_json::from_slice(&output.stdout).unwrap();
        codes(&diagnostics)
    };
    files
        .write_file("lib/dplyr/NAMESPACE", "export(select)\n")
        .unwrap();
    assert!(run().contains(&"RY010".to_string()));
    std::fs::write(&namespace, "export(filter)\n").unwrap();
    assert!(!run().contains(&"RY010".to_string()));
}

#[test]
fn package_local_module_and_dplyr_import_reach_lsp_pipeline() {
    let files = fixture();
    let mut command = Command::new(env!("CARGO_BIN_EXE_ry"));
    command
        .arg("server")
        .current_dir(files.root())
        .env("RY_NO_INSTALLED_LIBRARIES", "1");
    let mut client = JsonRpcProcess::spawn(&mut command).unwrap();
    let root_uri = file_uri(files.root()).unwrap();
    let source_uri = file_uri(&files.path("R/use.R")).unwrap();
    let initialize = client
        .request(
            "initialize",
            json!({
                "processId": null,
                "rootUri": root_uri,
                "capabilities": {},
                "workspaceFolders": [{"uri": root_uri, "name": "box fixture"}]
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
                    && message["params"]["diagnostics"]
                        .as_array()
                        .is_some_and(|diagnostics| {
                            diagnostics
                                .iter()
                                .any(|diagnostic| diagnostic["code"] == "RY118")
                        })
            },
            64,
        )
        .unwrap();
    let diagnostics = publication["params"]["diagnostics"].as_array().unwrap();
    let codes = codes(diagnostics);
    assert!(codes.contains(&"RY040".to_string()), "{diagnostics:#?}");
    assert!(!codes.contains(&"RY010".to_string()), "{diagnostics:#?}");
    let shutdown = client.request("shutdown", Value::Null).unwrap();
    client
        .receive_until(|message| message.get("id") == Some(&json!(shutdown)), 16)
        .unwrap();
    client.notify("exit", Value::Null).unwrap();
}

#[cfg(unix)]
#[test]
fn native_module_path_survives_lossy_display_collision_in_cli() {
    use std::os::unix::ffi::OsStringExt;

    let root = tempfile::tempdir().unwrap();
    let raw = root
        .path()
        .join(std::ffi::OsString::from_vec(b"bad\xff.r".to_vec()));
    let unicode = root.path().join("bad�.r");
    let caller = root.path().join("run.R");
    std::fs::write(&raw, "answer <- function() 'wrong'\n").unwrap();
    std::fs::write(&unicode, "answer <- function() 1L\n").unwrap();
    std::fs::write(
        &caller,
        "box::use(m = ./`bad�`)\nvalue <- m$answer() + 1L\n",
    )
    .unwrap();
    let check = |paths: &[&std::path::Path]| {
        let output = Command::new(env!("CARGO_BIN_EXE_ry"))
            .args(["check", "--output-format", "json", "--exit-zero"])
            .args(paths)
            .env("RY_NO_INSTALLED_LIBRARIES", "1")
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(0), "{output:?}");
        let diagnostics: Vec<Value> = serde_json::from_slice(&output.stdout).unwrap();
        assert!(
            diagnostics.iter().all(|diagnostic| {
                diagnostic["path"] != caller.to_string_lossy().as_ref()
                    || diagnostic["code"] != "RY040"
            }),
            "{diagnostics:#?}"
        );
    };
    check(&[root.path()]);
    check(&[&raw, &unicode, &caller]);
    check(&[&unicode, &raw, &caller]);
    std::fs::remove_file(&raw).unwrap();
    check(&[root.path()]);
}
