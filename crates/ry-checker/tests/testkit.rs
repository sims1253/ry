use ry_checker::Project;
use ry_core::RParser;
use ry_testkit::{
    Driver, DriverError, FixtureProject, ObservedDiagnostic, ObservedFix, ObservedPosition,
    ObservedRange, PositionEncoding, normalize_path,
};

struct ProjectDriver;

impl Driver for ProjectDriver {
    fn published_diagnostics(
        &mut self,
        fixture: &FixtureProject,
    ) -> Result<Vec<ObservedDiagnostic>, DriverError> {
        let mut parser = RParser::new()?;
        let mut project = Project::new();
        let mut sources = std::collections::HashMap::new();
        for path in fixture.files()? {
            if !matches!(
                path.extension().and_then(|extension| extension.to_str()),
                Some("R" | "r")
            ) {
                continue;
            }
            let relative = normalize_path(&path, fixture.root());
            let source = std::fs::read_to_string(&path)?;
            sources.insert(relative.clone(), source.clone());
            project.add_file(relative.clone(), parser.parse(&relative, &source)?);
        }
        Ok(project
            .check()
            .into_iter()
            .flat_map(|(_, diagnostics)| diagnostics)
            .map(|diagnostic| {
                let fix = diagnostic.fix.as_ref().and_then(|fix| {
                    let source = sources.get(&diagnostic.path)?;
                    Some(ObservedFix {
                        range: ObservedRange {
                            start: byte_offset_position(source, fix.span.start),
                            end: Some(byte_offset_position(source, fix.span.end)),
                        },
                        replacement: fix.replacement.clone(),
                    })
                });
                ObservedDiagnostic {
                    path: normalize_path(&diagnostic.path, fixture.root()),
                    code: diagnostic.code.to_string(),
                    severity: diagnostic.severity.as_str().to_string(),
                    message: diagnostic.message,
                    range: ObservedRange {
                        start: ObservedPosition {
                            line: diagnostic.span.line as u32,
                            character: diagnostic.span.col as u32,
                            encoding: PositionEncoding::Utf8Byte,
                        },
                        end: None,
                    },
                    confidence: Some(diagnostic.confidence.as_str().to_string()),
                    fix,
                }
            })
            .collect())
    }
}

fn byte_offset_position(source: &str, offset: usize) -> ObservedPosition {
    let prefix = &source[..offset];
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count() as u32;
    let line_start = prefix.rfind('\n').map_or(0, |position| position + 1);
    ObservedPosition {
        line,
        character: (offset - line_start) as u32,
        encoding: PositionEncoding::Utf8Byte,
    }
}

#[test]
fn shared_fixture_reaches_in_process_project_adapter() {
    let fixture = FixtureProject::from_fixture("shared").unwrap();
    let diagnostics = ProjectDriver.published_diagnostics(&fixture).unwrap();
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
