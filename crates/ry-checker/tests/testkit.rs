use ry_checker::Project;
use ry_core::RParser;
use ry_testkit::{DriverError, FixtureProject, normalize_path};

#[test]
fn shared_fixture_reaches_in_process_project_adapter() -> Result<(), DriverError> {
    let fixture = FixtureProject::from_fixture("shared")?;
    let mut parser = RParser::new()?;
    let mut project = Project::new();
    for path in fixture.files()? {
        if !matches!(
            path.extension().and_then(|extension| extension.to_str()),
            Some("R" | "r")
        ) {
            continue;
        }
        let relative = normalize_path(&path, fixture.root());
        let source = std::fs::read_to_string(&path)?;
        project.add_file(relative.clone(), parser.parse(&relative, &source)?);
    }
    let diagnostics: Vec<_> = project
        .check()
        .into_iter()
        .flat_map(|(_, diagnostics)| diagnostics)
        .collect();
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
    Ok(())
}
