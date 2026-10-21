//! Real CLI and LSP entry points for package-local box modules.
use ry_testkit::{CliProcess, FixtureProject, JsonRpcProcess, file_uri};
use serde_json::{Value, json};
use std::process::Command;

use crate::support::check;

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

/// Rule codes of a JSON check, after asserting its exit code.
fn codes(process: CliProcess, status: i32) -> Vec<String> {
    let output = process.args(["--output-format", "json"]).run();
    assert_eq!(output.code(), Some(status), "{output:?}");
    output.codes()
}

#[test]
fn package_local_module_and_dplyr_import_reach_cli_pipeline() {
    let codes = codes(check().arg(fixture().root()), 1);
    assert!(codes.contains(&"RY118".to_string()), "{codes:?}");
    assert!(codes.contains(&"RY040".to_string()), "{codes:?}");
    assert!(!codes.contains(&"RY010".to_string()), "{codes:?}");
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
        let process = check()
            .arg("--exit-zero")
            .arg(files.path("run.R"))
            .env_remove("RY_NO_INSTALLED_LIBRARIES")
            .env("R_LIBS", files.path("lib"))
            .env("R_LIBS_USER", files.path("missing-user-lib"))
            .env("R_LIBS_SITE", files.path("missing-site-lib"));
        codes(process, 0)
    };
    files
        .write_file("lib/dplyr/NAMESPACE", "export(select)\n")
        .unwrap();
    assert!(run().contains(&"RY010".to_string()));
    std::fs::write(&namespace, "export(filter)\n").unwrap();
    assert!(!run().contains(&"RY010".to_string()));
    // A pattern may export any stubbed name; literal exports cannot gate it.
    std::fs::write(&namespace, "export(select)\nexportPattern(\"^[^.]\")\n").unwrap();
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
    assert!(
        diagnostics.iter().any(|d| d["code"] == "RY040"),
        "{diagnostics:#?}"
    );
    assert!(
        diagnostics.iter().all(|d| d["code"] != "RY010"),
        "{diagnostics:#?}"
    );
    let shutdown = client.request("shutdown", Value::Null).unwrap();
    client
        .receive_until(|message| message.get("id") == Some(&json!(shutdown)), 16)
        .unwrap();
    client.notify("exit", Value::Null).unwrap();
}

#[cfg(unix)]
#[test]
fn native_caller_directory_drives_relative_module_lookup_in_cli() {
    use std::os::unix::ffi::OsStringExt;

    let root = tempfile::tempdir().unwrap();
    let raw_dir = root
        .path()
        .join(std::ffi::OsString::from_vec(b"raw\xff".to_vec()));
    let unicode_dir = root.path().join("raw�");
    std::fs::create_dir_all(&raw_dir).unwrap();
    std::fs::create_dir_all(&unicode_dir).unwrap();
    let caller = raw_dir.join("run.R");
    std::fs::write(&caller, "box::use(./mod[foo])\nfoo() + 1L\n").unwrap();
    std::fs::write(raw_dir.join("mod.r"), "foo <- function() 'wrong'\n").unwrap();
    std::fs::write(unicode_dir.join("mod.r"), "foo <- function() 'wrong'\n").unwrap();
    let check = |paths: &[&std::path::Path], expect_type_error: bool| {
        let codes = codes(check().arg("--exit-zero").args(paths), 0);
        assert_eq!(
            codes.contains(&"RY040".to_string()),
            expect_type_error,
            "{codes:?}"
        );
    };
    // A wrong return in the raw directory must be observed, proving that
    // the subsequent quiet case actually resolved that module.
    check(&[&caller], true);
    std::fs::write(raw_dir.join("mod.r"), "foo <- function() 1L\n").unwrap();
    check(&[&caller], false);
    check(&[root.path()], false);
    check(
        &[&caller, &unicode_dir.join("mod.r"), &raw_dir.join("mod.r")],
        false,
    );
    check(
        &[&unicode_dir.join("mod.r"), &raw_dir.join("mod.r"), &caller],
        false,
    );

    // Nested imports inherit the selected module's native directory.
    std::fs::write(
        raw_dir.join("mod.r"),
        "box::use(./inner[foo])\nbox::export(foo)\n",
    )
    .unwrap();
    std::fs::write(raw_dir.join("inner.r"), "foo <- function() 1L\n").unwrap();
    std::fs::write(unicode_dir.join("inner.r"), "foo <- function() 'wrong'\n").unwrap();
    check(&[root.path()], false);

    // When the native module disappears, the similar display spelling is
    // never a fallback source for this caller.
    std::fs::remove_file(raw_dir.join("mod.r")).unwrap();
    check(&[root.path()], false);
    std::fs::write(raw_dir.join("mod.r"), "foo <- function() 1L\n").unwrap();
    std::fs::remove_file(unicode_dir.join("mod.r")).unwrap();
    check(&[root.path()], false);
}
