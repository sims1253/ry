use ry_testkit::{
    CliProcess, Driver, DriverError, FixtureProject, ObservedDiagnostic, ObservedFix,
    ObservedPosition, ObservedRange, PositionEncoding, normalize_path,
};
use serde_json::Value;
use std::path::Path;

struct CliDriver {
    process: CliProcess,
}

impl Driver for CliDriver {
    fn published_diagnostics(
        &mut self,
        fixture: &FixtureProject,
    ) -> Result<Vec<ObservedDiagnostic>, DriverError> {
        let output = self.process.check(
            fixture,
            Path::new("R/diagnostic.R"),
            ["--output-format", "json"],
        )?;
        if !output.stderr.is_empty() {
            return Err(format!(
                "ry check wrote stderr: {}",
                String::from_utf8_lossy(&output.stderr)
            )
            .into());
        }
        let diagnostics: Vec<Value> = serde_json::from_slice(&output.stdout)?;
        diagnostics
            .into_iter()
            .map(|diagnostic| cli_diagnostic(diagnostic, fixture.root()))
            .collect()
    }
}

fn cli_diagnostic(value: Value, root: &Path) -> Result<ObservedDiagnostic, DriverError> {
    let line = required_u64(&value, "line")? as u32;
    let column = required_u64(&value, "column")? as u32;
    let path = required_str(&value, "path")?;
    let source_path = Path::new(path);
    let source_path = if source_path.is_absolute() {
        source_path.to_path_buf()
    } else {
        root.join(source_path)
    };
    let source = std::fs::read_to_string(source_path)?;
    let fix = value
        .get("fix")
        .map(|fix| -> Result<ObservedFix, DriverError> {
            Ok(ObservedFix {
                range: ObservedRange {
                    start: byte_offset_position(&source, required_u64(fix, "start")? as usize)?,
                    end: Some(byte_offset_position(
                        &source,
                        required_u64(fix, "end")? as usize,
                    )?),
                },
                replacement: required_str(fix, "replacement")?.to_string(),
            })
        })
        .transpose()?;
    Ok(ObservedDiagnostic {
        path: normalize_path(path, root),
        code: required_str(&value, "code")?.to_string(),
        severity: required_str(&value, "severity")?.to_string(),
        message: required_str(&value, "message")?.to_string(),
        range: ObservedRange {
            start: ObservedPosition {
                line: line.saturating_sub(1),
                character: column.saturating_sub(1),
                encoding: PositionEncoding::UnicodeScalar,
            },
            end: None,
        },
        confidence: value
            .get("confidence")
            .and_then(Value::as_str)
            .map(str::to_string),
        fix,
    })
}

fn byte_offset_position(source: &str, offset: usize) -> Result<ObservedPosition, DriverError> {
    if offset > source.len() || !source.is_char_boundary(offset) {
        return Err(format!("fix byte offset {offset} is not a source boundary").into());
    }
    let prefix = &source[..offset];
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count() as u32;
    let line_start = prefix.rfind('\n').map_or(0, |position| position + 1);
    Ok(ObservedPosition {
        line,
        character: (offset - line_start) as u32,
        encoding: PositionEncoding::Utf8Byte,
    })
}

fn required_str<'a>(value: &'a Value, key: &str) -> Result<&'a str, DriverError> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("diagnostic field `{key}` is not a string").into())
}

fn required_u64(value: &Value, key: &str) -> Result<u64, DriverError> {
    value
        .get(key)
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("diagnostic field `{key}` is not an integer").into())
}

#[test]
fn shared_fixture_reaches_real_cli_subprocess() {
    let fixture = FixtureProject::from_fixture("shared").unwrap();
    let diagnostics = CliDriver {
        process: CliProcess::new(env!("CARGO_BIN_EXE_ry")),
    }
    .published_diagnostics(&fixture)
    .unwrap();
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
