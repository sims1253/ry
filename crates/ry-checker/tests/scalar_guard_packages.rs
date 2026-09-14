//! Issue #372: the `length(x) == 1` guard exclusion for RY032 required the
//! bare `length` symbol to *strictly* resolve to base, which never holds
//! inside a package (the package search path shadows base). The exclusion
//! was dead code in package mode and 46 corpus guards warned. These
//! fixtures pin the four resolution toggles from the issue plus the
//! dispatch and reassignment refusals recorded in docs/scalar-guards.md.
use ry_checker::Project;
use ry_config::Config;
use ry_core::RParser;

const GUARD_SOURCE: &str = r#"
probe <- function(x) length(x) == 1L && x == ""
"#;

fn check_package(namespace: &str, source: &str) -> Vec<String> {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("R")).unwrap();
    std::fs::write(
        dir.path().join("DESCRIPTION"),
        "Package: fixture\nVersion: 0.0.0\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("NAMESPACE"), namespace).unwrap();
    let code_path = dir.path().join("R").join("code.R");
    let mut parser = RParser::new().unwrap();
    let file = parser
        .parse(code_path.to_str().unwrap(), source)
        .unwrap();
    let context = ry_workspace::resolve_workspace_context(
        dir.path(),
        &Config::default(),
        ry_workspace::ResolutionEnvironment {
            files: vec![&file],
            user_stubs: &Default::default(),
        },
    )
    .unwrap();
    let mut project = Project::new();
    project.add_file(code_path.to_string_lossy().into(), file);
    project.set_loaded(context.attached_packages);
    project.set_bare_loaded(context.bare_bindings);
    project.set_external_bindings(context.external_bindings);
    project.set_imported_from(context.imported_bindings);
    project.set_external_s3_methods(context.s3_methods);
    project
        .check()
        .into_iter()
        .flat_map(|(_, diagnostics)| diagnostics)
        .map(|diagnostic| diagnostic.code.to_string())
        .collect()
}

/// Toggle 1 from the issue: DESCRIPTION present (package mode) with a bare
/// `length` no longer kills the guard exclusion.
#[test]
fn bare_length_guard_exclusion_holds_in_package_mode() {
    let codes = check_package("", GUARD_SOURCE);
    assert!(!codes.contains(&"RY032".to_string()), "{codes:?}");
}

/// Toggles 3 and 4: an explicit `base::` qualification and an
/// `importFrom(base, length)` keep the exclusion alive as before.
#[test]
fn qualified_and_imported_length_guards_hold_in_package_mode() {
    let qualified = check_package(
        "",
        "probe <- function(x) base::length(x) == 1L && x == \"\"\n",
    );
    assert!(!qualified.contains(&"RY032".to_string()), "{qualified:?}");
    let imported = check_package("importFrom(base, length)\n", GUARD_SOURCE);
    assert!(!imported.contains(&"RY032".to_string()), "{imported:?}");
}

/// The sibling guard forms are unaffected: `length(x) > 0` reaches its
/// operand for lengths above one, so the warning still fires.
#[test]
fn non_equality_length_guards_still_warn_in_package_mode() {
    let codes = check_package(
        "",
        "probe <- function(x) length(x) > 0 && x == \"\"\n",
    );
    assert!(codes.contains(&"RY032".to_string()), "{codes:?}");
}

/// The exclusion still yields to soundness: a package-local `length`
/// function, or a `length.<class>` S3 method the guarded parameter could
/// dispatch to, keeps the warning alive.
#[test]
fn shadowed_length_and_dispatch_risk_still_warn_in_package_mode() {
    let shadowed = check_package(
        "",
        "length <- function(x) 1L\nprobe <- function(x) length(x) == 1L && x == \"\"\n",
    );
    assert!(shadowed.contains(&"RY032".to_string()), "{shadowed:?}");
    let dispatched = check_package(
        "S3method(length, disguised)\n",
        "length.disguised <- function(x) 1L\nprobe <- function(x) length(x) == 1L && x == \"\"\n",
    );
    assert!(dispatched.contains(&"RY032".to_string()), "{dispatched:?}");
    // The method need not be registered in NAMESPACE to be a risk.
    let unregistered = check_package(
        "",
        "length.disguised <- function(x) 1L\nprobe <- function(x) length(x) == 1L && x == \"\"\n",
    );
    assert!(
        unregistered.contains(&"RY032".to_string()),
        "{unregistered:?}"
    );
}

/// Reassignment inside the guarded operand invalidates the scalar proof.
#[test]
fn reassigned_guarded_operand_still_warns_in_package_mode() {
    let codes = check_package(
        "",
        "probe <- function(x) length(x) == 1L && { x <- c(1, 2); x == 1L }\n",
    );
    assert!(codes.contains(&"RY032".to_string()), "{codes:?}");
}
