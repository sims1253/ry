//! File classification for R packages.

/// Classification of a file within an R package structure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageFileKind {
    Library,
    TestCode,
    TestFixture,
    Inst,
    Other,
}

/// Classify a path relative to its nearest ancestor containing `DESCRIPTION`.
/// Testthat only sources runner files at `tests/` root and files with its
/// executable prefixes directly under `tests/testthat/`; deeper R files are
/// data consumed by tests, not code executed by the package test runner.
pub fn package_file_kind(path: &std::path::Path) -> PackageFileKind {
    let Some(root) = path
        .parent()
        .and_then(|parent| parent.ancestors().find(|p| p.join("DESCRIPTION").is_file()))
    else {
        return PackageFileKind::Other;
    };
    let Ok(relative) = path.strip_prefix(root) else {
        return PackageFileKind::Other;
    };
    let components: Vec<_> = relative
        .components()
        .filter_map(|component| component.as_os_str().to_str())
        .collect();
    match components.as_slice() {
        ["R", _] => PackageFileKind::Library,
        ["inst", ..] => PackageFileKind::Inst,
        ["tests", file] if is_r_source_name(file) => PackageFileKind::TestCode,
        ["tests", "testthat", file] if is_r_source_name(file) && is_testthat_code_name(file) => {
            PackageFileKind::TestCode
        }
        ["tests", ..] => PackageFileKind::TestFixture,
        _ => PackageFileKind::Other,
    }
}

fn is_r_source_name(name: &str) -> bool {
    std::path::Path::new(name)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| matches!(extension, "R" | "r" | "S" | "s" | "q"))
}

fn is_testthat_code_name(name: &str) -> bool {
    let stem = std::path::Path::new(name)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or(name);
    ["test", "helper", "setup", "teardown"]
        .iter()
        .any(|prefix| stem.starts_with(prefix))
}
