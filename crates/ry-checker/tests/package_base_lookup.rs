//! Issue #568: package code finds base before the search path, so the
//! package itself and its Depends cannot mask a base special such as
//! `switch`. The reprex audit sites pass a package-local NULL return into
//! `switch`. A whole-package `import()` may supply any name ahead of base,
//! so such packages keep the conservative search-path guard.
use ry_checker::Project;
use ry_config::Config;
use ry_core::RParser;

fn check_package(namespace: &str, files: &[(&str, &str)]) -> Vec<String> {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("R")).unwrap();
    std::fs::write(
        dir.path().join("DESCRIPTION"),
        "Package: fixture\nVersion: 0.0.0\nDepends: R (>= 4.1)\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("NAMESPACE"), namespace).unwrap();
    let mut parser = RParser::new().unwrap();
    let parsed: Vec<_> = files
        .iter()
        .map(|(name, source)| {
            let path = dir.path().join("R").join(name);
            (
                path.to_string_lossy().into_owned(),
                parser.parse(path.to_str().unwrap(), source).unwrap(),
            )
        })
        .collect();
    let context = ry_workspace::resolve_workspace_context(
        dir.path(),
        &Config::default(),
        ry_workspace::ResolutionEnvironment {
            files: parsed.iter().map(|(_, file)| file).collect(),
            user_stubs: &Default::default(),
        },
    )
    .unwrap();
    let mut project = Project::new();
    for (path, file) in parsed {
        project.add_file(path, file);
    }
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

const LOCATE: &str = "locate <- function(x) if (is.null(x)) NULL else \"path\"\n";

#[test]
fn package_local_null_return_reaches_switch() {
    let consume = "consume <- function(x = NULL) {\n  where <- locate(x)\n  switch(where, path = \"ok\")\n}\n";
    let codes = check_package(
        "export(consume)\nimportFrom(glue, glue)\n",
        &[("locate.R", LOCATE), ("consume.R", consume)],
    );
    assert_eq!(codes, ["RY001"]);
    let codes = check_package("", &[("f.R", "f <- function() switch(NULL, a = 1)\n")]);
    assert_eq!(codes, ["RY001"]);
}

#[test]
fn guarded_or_masked_switch_stays_silent() {
    let guarded = "consume <- function(x = NULL) {\n  where <- locate(x)\n  if (is.null(where)) return(NULL)\n  switch(where, path = \"ok\")\n}\n";
    let codes = check_package("", &[("locate.R", LOCATE), ("consume.R", guarded)]);
    assert!(codes.is_empty(), "{codes:?}");
    // A package-local definition masks base.
    let local = "switch <- function(EXPR, ...) NULL\nf <- function() switch(NULL, a = 1)\n";
    let codes = check_package("", &[("f.R", local)]);
    assert!(!codes.contains(&"RY001".to_string()), "{codes:?}");
    // A whole-package import may supply `switch` first.
    let codes = check_package(
        "import(notInstalledFixturePkg)\n",
        &[("f.R", "f <- function() switch(NULL, a = 1)\n")],
    );
    assert!(!codes.contains(&"RY001".to_string()), "{codes:?}");
}
