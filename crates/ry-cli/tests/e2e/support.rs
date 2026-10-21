//! Helpers used by more than one suite.

use std::fs;
use std::path::Path;

use ry_testkit::{CliProcess, ry};
use serde_json::Value;

/// `ry check` that ignores the machine's installed R libraries. Tests that
/// install a fixture library call `.env_remove("RY_NO_INSTALLED_LIBRARIES")`.
pub fn check() -> CliProcess {
    ry!().arg("check").env("RY_NO_INSTALLED_LIBRARIES", "1")
}

/// `(relative path, contents)` pairs to create.
pub type Files<'a> = &'a [(&'a str, &'a [u8])];

/// Create `files` under `root`, with parent directories.
pub fn write(root: &Path, files: Files) {
    for (path, contents) in files {
        let path = root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }
}

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
