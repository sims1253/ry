use ry_testkit::{
    Driver, DriverError, FixtureProject, JsonRpcProcess, ObservedDiagnostic, ObservedFix,
    ObservedPosition, ObservedRange, PositionEncoding, normalize_path,
};
use serde_json::{Value, json};
use std::path::Path;
use std::process::Command;

struct LspSubprocessDriver;

impl Driver for LspSubprocessDriver {
    fn published_diagnostics(
        &mut self,
        fixture: &FixtureProject,
    ) -> Result<Vec<ObservedDiagnostic>, DriverError> {
        let mut command = Command::new(env!("CARGO_BIN_EXE_ry"));
        command.arg("server").current_dir(fixture.root());
        let mut client = JsonRpcProcess::spawn(&mut command)?;
        let root_uri = file_uri(fixture.root());
        let initialize_id = client.request(
            "initialize",
            json!({
                "processId": null,
                "rootUri": root_uri,
                "capabilities": {},
                "workspaceFolders": [{"uri": root_uri, "name": "shared"}]
            }),
        )?;
        client.receive_until(
            |message| message.get("id") == Some(&json!(initialize_id)),
            8,
        )?;
        client.notify("initialized", json!({}))?;

        let path = fixture.path("R/diagnostic.R");
        let uri = file_uri(&path);
        let text = std::fs::read_to_string(&path)?;
        client.notify(
            "textDocument/didOpen",
            json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": "r",
                    "version": 1,
                    "text": text,
                }
            }),
        )?;
        let publish = client.receive_until(
            |message| {
                message.get("method") == Some(&json!("textDocument/publishDiagnostics"))
                    && message.pointer("/params/uri") == Some(&json!(uri))
            },
            32,
        )?;
        let diagnostics = lsp_diagnostics(&publish, &path, fixture.root())?;

        let shutdown_id = client.request("shutdown", Value::Null)?;
        client.receive_until(|message| message.get("id") == Some(&json!(shutdown_id)), 8)?;
        client.notify("exit", Value::Null)?;
        Ok(diagnostics)
    }
}

fn lsp_diagnostics(
    publish: &Value,
    path: &Path,
    root: &Path,
) -> Result<Vec<ObservedDiagnostic>, DriverError> {
    publish
        .pointer("/params/diagnostics")
        .and_then(Value::as_array)
        .ok_or_else(|| -> DriverError { "publishDiagnostics has no diagnostics array".into() })?
        .iter()
        .map(|value| {
            let start = value
                .pointer("/range/start")
                .ok_or("diagnostic has no start")?;
            let end = value.pointer("/range/end").ok_or("diagnostic has no end")?;
            Ok(ObservedDiagnostic {
                path: normalize_path(path, root),
                code: value
                    .get("code")
                    .and_then(Value::as_str)
                    .ok_or("diagnostic has no string code")?
                    .to_string(),
                severity: match value.get("severity").and_then(Value::as_u64) {
                    Some(1) => "error",
                    Some(2) => "warning",
                    Some(3) => "information",
                    Some(4) => "hint",
                    _ => "unknown",
                }
                .to_string(),
                message: value
                    .get("message")
                    .and_then(Value::as_str)
                    .ok_or("diagnostic has no message")?
                    .to_string(),
                range: ObservedRange {
                    start: lsp_position(start)?,
                    end: Some(lsp_position(end)?),
                },
                confidence: None,
                fix: lsp_fix(value)?,
            })
        })
        .collect()
}

fn lsp_fix(value: &Value) -> Result<Option<ObservedFix>, DriverError> {
    value
        .pointer("/data/fix")
        .map(|fix| {
            Ok(ObservedFix {
                range: ObservedRange {
                    start: lsp_position(&fix["range"]["start"])?,
                    end: Some(lsp_position(&fix["range"]["end"])?),
                },
                replacement: fix["replacement"]
                    .as_str()
                    .ok_or("fix replacement is not a string")?
                    .to_string(),
            })
        })
        .transpose()
}

fn lsp_position(value: &Value) -> Result<ObservedPosition, DriverError> {
    Ok(ObservedPosition {
        line: value
            .get("line")
            .and_then(Value::as_u64)
            .ok_or("position has no line")? as u32,
        character: value
            .get("character")
            .and_then(Value::as_u64)
            .ok_or("position has no character")? as u32,
        encoding: PositionEncoding::Utf16,
    })
}

fn file_uri(path: &Path) -> String {
    format!("file://{}", path.display())
}

#[test]
fn shared_fixture_reaches_real_lsp_subprocess_without_stdout_corruption() {
    let fixture = FixtureProject::from_fixture("shared").unwrap();
    let diagnostics = LspSubprocessDriver.published_diagnostics(&fixture).unwrap();
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "RY002"),
        "shared fixture should publish RY002: {diagnostics:?}"
    );
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.path == "R/diagnostic.R")
    );
}
