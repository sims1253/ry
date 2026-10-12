//! Helpers used by more than one suite.

use std::fs;
use std::path::Path;

use serde_json::Value;

/// Names reported unbound (RY010) by a JSON check, in output order. Any
/// other diagnostic fails the assertion.
pub fn unbound_names(diagnostics: &[Value]) -> Vec<String> {
    diagnostics
        .iter()
        .map(|diagnostic| {
            assert_eq!(diagnostic["code"], "RY010", "{diagnostic}");
            let message = diagnostic["message"].as_str().unwrap();
            message.split('`').nth(1).unwrap().to_owned()
        })
        .collect()
}

/// A statically installed `package` in `library` whose NAMESPACE exports
/// `export`.
pub fn install_namespace(library: &Path, package: &str, export: &str) {
    let package = library.join(package);
    fs::create_dir_all(&package).unwrap();
    fs::write(package.join("NAMESPACE"), format!("export({export})\n")).unwrap();
}
