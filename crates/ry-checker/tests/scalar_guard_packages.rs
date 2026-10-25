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
    check_package_diagnostics(namespace, source)
        .into_iter()
        .map(|diagnostic| diagnostic.code.to_string())
        .collect()
}

fn check_package_diagnostics(namespace: &str, source: &str) -> Vec<ry_checker::Diagnostic> {
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
    let file = parser.parse(code_path.to_str().unwrap(), source).unwrap();
    let context = ry_workspace::resolve_workspace_context(
        dir.path(),
        &Config::default(),
        vec![&file],
        &Default::default(),
    )
    .unwrap();
    let mut project = Project::new();
    project.add_file(code_path.to_string_lossy().into(), file);
    project.set_loaded(context.attached_packages);
    project.set_bare_loaded(context.bare_bindings);
    project.set_external_bindings(context.external_bindings);
    project.set_package_facts(context.package_facts);
    project.set_imported_from(context.imported_bindings);
    project.set_external_s3_methods(context.s3_methods);
    project
        .check()
        .into_iter()
        .flat_map(|(_, diagnostics)| diagnostics)
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
    let codes = check_package("", "probe <- function(x) length(x) > 0 && x == \"\"\n");
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
    // A backtick-quoted definition binds the same `length`.
    let quoted = check_package(
        "",
        "`length` <- function(x) 1L\nprobe <- function(x) length(x) == 1L && x == \"\"\n",
    );
    assert!(quoted.contains(&"RY032".to_string()), "{quoted:?}");
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

/// R skips a data binding when it looks up a call head, so a top-level
/// `length <- 0L` leaves the guard calling base `length`.
#[test]
fn data_named_length_keeps_the_guard() {
    let codes = check_package(
        "",
        "length <- 0L\nprobe <- function(x) length(x) == 1L && x == \"\"\n",
    );
    assert!(!codes.contains(&"RY032".to_string()), "{codes:?}");
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

/// Issue #701: a method that reports the storage length cannot make the
/// guard hold for a longer object, so it leaves the guard trusted.
#[test]
fn storage_length_method_keeps_the_guard() {
    for method in [
        "length.cell <- function(x) length(unclass(x))\n",
        "length.cell <- function(object) {\n  length(unclass(object))\n}\n",
    ] {
        let codes = check_package(
            "S3method(length, cell)\n",
            &format!("{method}{}", GUARD_SOURCE),
        );
        assert!(!codes.contains(&"RY032".to_string()), "{method}: {codes:?}");
    }
}

/// Any other method, or a masked `unclass`, still distrusts the guard, and
/// the warning names the method that did it (#701).
#[test]
fn distrusted_guard_names_the_length_method() {
    let source = "length.cell <- function(x) length(unclass(x))\n\
                  length.doc <- function(x) {\n  length(x$body)\n}\n\
                  probe <- function(x) length(x) == 1L && x == \"\"\n";
    let diagnostics =
        check_package_diagnostics("S3method(length, cell)\nS3method(length, doc)\n", source);
    let [warning] = &diagnostics[..] else {
        panic!("{diagnostics:?}");
    };
    assert_eq!(warning.code, "RY032");
    assert!(
        warning.message.contains(
            "the `length(x) == 1` guard is not trusted because `length.doc` may report 1 for a longer object"
        ),
        "{}",
        warning.message
    );
    let [related] = &warning.related[..] else {
        panic!("{:?}", warning.related);
    };
    assert!(related.path.ends_with("code.R"), "{}", related.path);
    assert_eq!(related.span.start, source.find("function(x) {").unwrap());
    assert_eq!(related.message, "`length.doc` is defined here");

    // A masked `unclass`, in any spelling, can make the method lie.
    for mask in [
        "unclass <- function(x) x[1L]",
        "`unclass` <- function(x) x[1L]",
        "\"unclass\" <- function(x) x[1L]",
    ] {
        let masked = check_package_diagnostics(
            "S3method(length, cell)\n",
            &format!(
                "{mask}\nlength.cell <- function(x) length(unclass(x))\nprobe <- function(x) length(x) == 1L && x == \"\"\n"
            ),
        );
        assert!(
            masked
                .iter()
                .any(|d| d.code == "RY032" && d.message.contains("`length.cell` may report 1")),
            "{mask}: {masked:?}"
        );
    }
    // A registration with no project definition names the method without
    // a location.
    // So does an imported method, whose body ry cannot read either.
    for namespace in [
        "S3method(length, remote)\n",
        "importFrom(otherpkg, length.remote)\n",
    ] {
        let unseen = check_package_diagnostics(namespace, GUARD_SOURCE);
        let [warning] = &unseen[..] else {
            panic!("{namespace}: {unseen:?}");
        };
        assert!(
            warning.code == "RY032" && warning.message.contains("`length.remote` may report 1"),
            "{namespace}: {}",
            warning.message
        );
        assert!(warning.related.is_empty(), "{:?}", warning.related);
    }
}
