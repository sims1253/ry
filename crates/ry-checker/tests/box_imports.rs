//! box::use module support. Caller fixtures under `testdata/box` carry a
//! `# expect:` or `# no-diag` marker on their first line and import modules
//! beside them or from `testdata/oracle/box_module`. The Rust tests below
//! cover what fixtures cannot express: project overlays, incremental checks,
//! symlinks, non-UTF-8 filenames, case-colliding module spellings, and the
//! module size limit.

use ry_checker::{Checker, Project};
use ry_core::{RParser, SourceFile};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn parse(path: &Path, source: &str) -> SourceFile {
    let mut parser = RParser::new().unwrap();
    let parsed = parser.parse(&path.to_string_lossy(), source).unwrap();
    assert!(parsed.parse_errors.is_empty(), "{:?}", parsed.parse_errors);
    parsed
}

fn add(project: &mut Project, path: &Path, source: &str) {
    project.add_file(path.to_string_lossy().into_owned(), parse(path, source));
}

fn codes(path: &Path, source: &str) -> Vec<String> {
    let mut checker = Checker::new(&path.to_string_lossy());
    let mut codes: Vec<String> = checker
        .check(&parse(path, source))
        .iter()
        .map(|diagnostic| diagnostic.code.to_string())
        .collect();
    codes.sort();
    codes
}

fn has(diagnostics: &[ry_checker::diagnostics::Diagnostic], code: &str) -> bool {
    diagnostics.iter().any(|diagnostic| diagnostic.code == code)
}

/// The sorted codes a caller fixture expects, or `None` for a module.
fn expected_codes(source: &str) -> Option<Vec<String>> {
    let marker = source.lines().next()?.strip_prefix('#')?.trim();
    if marker == "no-diag" {
        return Some(Vec::new());
    }
    let mut codes: Vec<String> = marker
        .strip_prefix("expect:")?
        .split([',', ' '])
        .filter(|code| !code.is_empty())
        .map(str::to_string)
        .collect();
    codes.sort();
    Some(codes)
}

fn fixture_files(dir: &Path, files: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            fixture_files(&path, files);
        } else if path
            .extension()
            .is_some_and(|extension| extension == "R" || extension == "r")
        {
            files.push(path);
        }
    }
}

#[test]
fn box_fixtures() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata/box");
    let mut files = Vec::new();
    fixture_files(&root, &mut files);
    files.sort();
    let mut callers = 0;
    let mut failures = BTreeMap::new();
    for path in files {
        let source = fs::read_to_string(&path).unwrap();
        let Some(expected) = expected_codes(&source) else {
            continue;
        };
        callers += 1;
        let mut checker = Checker::new(&path.to_string_lossy());
        let diagnostics = checker.check(&parse(&path, &source));
        let mut found: Vec<String> = diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code.to_string())
            .collect();
        found.sort();
        if found != expected {
            let name = path.strip_prefix(&root).unwrap().display().to_string();
            let details: Vec<String> = diagnostics
                .iter()
                .map(|diagnostic| {
                    let line = diagnostic.span.line + 1;
                    format!("{line}: {} {}", diagnostic.code, diagnostic.message)
                })
                .collect();
            failures.insert(name, (expected, details));
        }
    }
    assert!(callers > 0, "no caller fixtures under {}", root.display());
    assert!(failures.is_empty(), "(expected, found): {failures:#?}");
}

/// Kept out of the caller fixtures: the cloned-scope reference that the
/// journal parity test walks over `testdata` does not join box provenance.
#[test]
fn branch_joins_keep_box_provenance_only_when_paths_agree() {
    let caller = Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata/box/branches.R");
    let check = |before: &str, then: &str, else_: &str, after: &str| {
        let source = format!(
            "{before}\nif (runif(1) > 0.5) {{\n  box::use({then})\n}} else {{\n  box::use({else_})\n}}\n{after}\n"
        );
        codes(&caller, &source)
    };
    let int = "./modules/int_function";
    let text = "./modules/text_function";
    // Agreeing paths replace the earlier object or attachment.
    let objects = check(
        &format!("box::use(m = {int})"),
        &format!("m = {text}"),
        &format!("m = {text}"),
        "m$foo() + 1L",
    );
    assert_eq!(objects, ["RY040"]);
    let attachments = check(
        &format!("box::use({int}[foo])"),
        &format!("{text}[foo]"),
        &format!("{text}[foo]"),
        "foo() + 1L",
    );
    assert_eq!(attachments, ["RY040"]);
    // Disagreeing paths leave calls through the name opaque.
    let either = check(
        "",
        &format!("{int}[foo]"),
        &format!("{text}[foo]"),
        "foo() + 1L",
    );
    assert_eq!(either, Vec::<String>::new());
}

#[test]
fn project_module_overlay_replaces_disk_inventory_on_incremental_check() {
    let root = tempfile::tempdir().unwrap();
    let module = root.path().join("mod/hello.r");
    fs::create_dir_all(module.parent().unwrap()).unwrap();
    fs::write(&module, "foo <- 1L\n").unwrap();
    let caller = root.path().join("run.R");
    let mut project = Project::new();
    add(&mut project, &module, "foo <- 1L\n");
    add(
        &mut project,
        &caller,
        "box::use(./mod/hello[foo, bar])\nfoo\nbar\n",
    );
    let first = project.check();
    assert!(has(&first[1].1, "RY118"));
    project.update_file(
        module.to_string_lossy().into_owned(),
        Arc::new(parse(&module, "foo <- 1L\nbar <- 2L\n")),
    );
    let second = project.check_incremental();
    assert!(!has(&second[1].1, "RY118"));
    let cold = project.check();
    assert_eq!(second, cold, "warm importers must match a cold inventory");
    project.remove_file(&module.to_string_lossy());
    fs::remove_file(&module).unwrap();
    let missing = project.check_incremental();
    assert!(!has(&missing[0].1, "RY118"));
}

#[test]
fn project_module_overlays_are_used_unless_r_cannot_parse_them() {
    let root = tempfile::tempdir().unwrap();
    let module = root.path().join("mod.r");
    let caller = root.path().join("run.R");
    // An unsaved module is read from its buffer, including one beyond the
    // size limit for disk reads, and a spaced `box :: use` still marks the
    // caller as an importer.
    let large = format!("foo <- 1L\n{}\n", " ".repeat(1_048_577));
    for source in ["foo <- 1L\n", large.as_str()] {
        let mut project = Project::new();
        add(&mut project, &module, source);
        add(
            &mut project,
            &caller,
            "box :: use(./mod[foo, missing])\nfoo\n",
        );
        assert!(has(&project.check()[1].1, "RY118"));
    }
    for boundary in ["bom", "invalid_utf8"] {
        let mut file = parse(&module, "foo <- 1L\n");
        if boundary == "bom" {
            file.leading_bom = true;
        } else {
            file.invalid_utf8 = vec![ry_core::Span::default()];
        }
        let mut project = Project::new();
        project.add_file(module.to_string_lossy().into_owned(), file);
        add(
            &mut project,
            &caller,
            "box::use(./mod[foo, missing])\nfoo\n",
        );
        let result = project.check();
        assert!(
            !has(&result[1].1, "RY118"),
            "{boundary}: {:#?}",
            result[1].1
        );
    }
}

#[test]
fn ordinary_box_text_does_not_invalidate_unrelated_project_files() {
    let root = tempfile::tempdir().unwrap();
    let a = root.path().join("a.R");
    let b = root.path().join("b.R");
    let mut project = Project::new();
    add(&mut project, &a, "x <- 1L # toolbox text\n");
    add(&mut project, &b, "boxplot(1:3)\n");
    project.check();
    assert_eq!(project.emit_count, 2);
    project.update_file(
        a.to_string_lossy().into_owned(),
        Arc::new(parse(&a, "x <- 2L # toolbox text\n")),
    );
    let warm = project.check_incremental();
    assert_eq!(project.emit_count, 1, "unrelated file stays cached");
    assert_eq!(warm, project.check(), "warm and cold results agree");
}

#[test]
fn open_buffers_outside_the_project_hide_their_disk_modules() {
    let root = tempfile::tempdir().unwrap();
    let module = root.path().join("mod.r");
    let caller = root.path().join("run.R");
    fs::write(&module, "foo <- 1L\n").unwrap();
    for open in [false, true] {
        let mut project = Project::new();
        add(
            &mut project,
            &caller,
            "box::use(./mod[foo, missing])\nfoo\n",
        );
        if open {
            project.set_open_buffer_paths([module.to_string_lossy().into_owned()].into());
        }
        let result = project.check();
        assert_eq!(
            has(&result[0].1, "RY118"),
            !open,
            "open={open}: {:#?}",
            result[0].1
        );
    }
}

#[test]
fn module_size_and_spelling_limits_never_select_another_file() {
    let root = tempfile::tempdir().unwrap();
    let caller = root.path().join("run.R");
    // When both spellings exist, box loads the lowercase `.r`.
    fs::write(root.path().join("mod.r"), "lower <- 1L\n").unwrap();
    fs::write(root.path().join("mod.R"), "upper <- 2L\n").unwrap();
    if fs::read_dir(root.path()).unwrap().count() != 2 {
        return; // a case-insensitive filesystem cannot hold both spellings
    }
    assert_eq!(
        codes(&caller, "box::use(./mod[lower])\nlower\n"),
        Vec::<String>::new()
    );
    assert_eq!(codes(&caller, "box::use(./mod[upper])\n"), ["RY118"]);
    // An oversized or unreadable preferred module is opaque; `.R` would
    // describe a different module.
    let source = "box::use(./mod[foo, missing])\nfoo() + 1L\n";
    fs::write(root.path().join("mod.R"), "foo <- function() 'wrong'\n").unwrap();
    fs::write(root.path().join("mod.r"), vec![b' '; 1_048_577]).unwrap();
    assert_eq!(codes(&caller, source), Vec::<String>::new());
    fs::write(root.path().join("mod.r"), [0xffu8]).unwrap();
    assert_eq!(codes(&caller, source), Vec::<String>::new());
    fs::remove_file(root.path().join("mod.r")).unwrap();
    assert_eq!(codes(&caller, source), ["RY040", "RY118"]);
}

#[cfg(unix)]
#[test]
fn unsaved_module_overlay_matches_symlinked_caller_root() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().unwrap();
    let actual = root.path().join("real");
    fs::create_dir(&actual).unwrap();
    let alias = root.path().join("alias");
    symlink(&actual, &alias).unwrap();
    let module = actual.join("unsaved.r");
    let mut project = Project::new();
    add(&mut project, &module, "foo <- function() 1L\n");
    add(
        &mut project,
        &alias.join("run.R"),
        "box::use(./unsaved[foo, missing])\nfoo()\n",
    );
    assert!(has(&project.check()[1].1, "RY118"));
    assert!(
        !module.exists(),
        "the editor overlay must not write the module"
    );
}

#[cfg(unix)]
#[test]
fn symlinked_modules_resolve_imports_beside_their_target() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("lib")).unwrap();
    let target = root.path().join("lib/mod.r");
    let module_source = "box::use(./dep[f])\nbox::export(f)\n";
    fs::write(&target, module_source).unwrap();
    fs::write(root.path().join("lib/dep.r"), "f <- function() 1L\n").unwrap();
    fs::write(root.path().join("dep.r"), "f <- function() 'wrong'\n").unwrap();
    let link = root.path().join("link.r");
    symlink(&target, &link).unwrap();
    let caller = root.path().join("run.R");
    let source = "box::use(./link[f])\nf() + 1L\n";
    assert_eq!(codes(&caller, source), Vec::<String>::new(), "disk");
    // An overlay reached through the link resolves the same way.
    let mut project = Project::new();
    add(&mut project, &link, module_source);
    add(&mut project, &caller, source);
    let result = project.check();
    assert!(!has(&result[1].1, "RY040"), "overlay: {:#?}", result[1].1);
}

#[cfg(unix)]
#[test]
fn unsaved_unicode_module_overlay_does_not_borrow_raw_filename_source() {
    use std::os::unix::ffi::OsStringExt;

    let root = tempfile::tempdir().unwrap();
    let raw = root
        .path()
        .join(std::ffi::OsString::from_vec(b"bad\xff.r".to_vec()));
    let unicode = root.path().join("bad�.r");
    let caller = root.path().join("run.R");
    fs::write(&raw, "answer <- function() 'wrong'\n").unwrap();
    fs::write(&unicode, "answer <- function() 'disk is stale'\n").unwrap();
    for reverse in [false, true] {
        let mut raw_file = parse(&raw, "answer <- function() 'wrong'\n");
        raw_file.native_path = Some(raw.clone());
        let mut files = vec![
            (raw.to_string_lossy().into_owned(), raw_file),
            (
                unicode.to_string_lossy().into_owned(),
                parse(&unicode, "answer <- function() 1L\n"),
            ),
        ];
        if reverse {
            files.reverse();
        }
        let mut project = Project::new();
        for (path, file) in files {
            project.add_file(path, file);
        }
        add(
            &mut project,
            &caller,
            "box::use(m = ./`bad�`)\nm$answer() + 1L\n",
        );
        let checked = project.check();
        let diagnostics = &checked.last().unwrap().1;
        assert!(
            !has(diagnostics, "RY040"),
            "reverse={reverse}: {diagnostics:#?}"
        );
    }
}
