use ry_checker::{Checker, Project};
use ry_core::{Mode, RParser, SourceFile};
use std::fs;
use std::path::Path;
use std::sync::Arc;

type Findings = Vec<(String, usize, String)>;

fn parse(path: &Path, source: &str) -> SourceFile {
    let mut parser = RParser::new().unwrap();
    let parsed = parser.parse(&path.to_string_lossy(), source).unwrap();
    assert!(parsed.parse_errors.is_empty(), "{:?}", parsed.parse_errors);
    parsed
}

fn write(root: &Path, name: &str, contents: impl AsRef<[u8]>) {
    fs::write(root.join(name), contents).unwrap();
}

fn add(project: &mut Project, path: &Path, source: &str) {
    project.add_file(path.to_string_lossy().into_owned(), parse(path, source));
}

fn findings(diagnostics: &[ry_checker::diagnostics::Diagnostic]) -> Findings {
    diagnostics
        .iter()
        .map(|diagnostic| {
            (
                diagnostic.code.to_string(),
                diagnostic.span.line,
                diagnostic.message.clone(),
            )
        })
        .collect()
}

fn codes_for(root: &Path, source: &str) -> Findings {
    let caller = root.join("run.R");
    let mut checker = Checker::new(&caller.to_string_lossy());
    findings(checker.check(&parse(&caller, source)))
}

fn codes(findings: &Findings) -> Vec<&str> {
    findings.iter().map(|(code, _, _)| code.as_str()).collect()
}

fn has(findings: &Findings, code: &str) -> bool {
    findings.iter().any(|(found, _, _)| found == code)
}

fn messages<'a>(findings: &'a Findings, code: &str) -> Vec<&'a str> {
    findings
        .iter()
        .filter(|(found, _, _)| found == code)
        .map(|(_, _, message)| message.as_str())
        .collect()
}

fn lines(findings: &Findings, code: &str) -> Vec<usize> {
    findings
        .iter()
        .filter(|(found, _, _)| found == code)
        .map(|(_, line, _)| *line)
        .collect()
}

#[test]
fn box_modules_bind_objects_selected_names_and_exports() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("mod")).unwrap();
    write(
        root.path(),
        "mod/hello.r",
        "#' @export\nsay_hello <- function(name) paste0('hi ', name)\nnot_exported <- function() 1L\n",
    );
    let caller = root.path().join("run.R");
    let caller_source = "box::use(./mod/hello)\nbox::use(./mod/hello[say_hello])\nhello$say_hello('a')\nsay_hello('a')\nbox::use(./mod/hello[missing])\n";
    let mut checker = Checker::new(&caller.to_string_lossy());
    let (diagnostics, scope) = checker.check_with_scope(&parse(&caller, caller_source));
    let diagnostics = findings(&diagnostics);
    assert!(has(&diagnostics, "RY118"));
    assert!(!has(&diagnostics, "RY010"));
    assert_eq!(scope.get("hello").unwrap().mode, Mode::Opaque);
}

#[test]
fn quoted_module_members_preserve_values_calls_and_proven_absence() {
    let root = tempfile::tempdir().unwrap();
    write(
        root.path(),
        "mod.r",
        "foo <- function() 1L\ntext <- function() 'wrong'\nlabel <- 'label'\nbox::export(foo, text, label)\n",
    );
    write(
        root.path(),
        "renamed.r",
        "#' @export\nbox::use(./mod[\"my-fn\" = foo, \"my-text\" = text])\n",
    );
    let mut cases: Vec<(String, &[&str])> = Vec::new();
    for member in ["foo", "`foo`", "\"foo\"", "`\\x66oo`", "\"\\u0066oo\""] {
        cases.push((
            format!("box::use(m = ./mod)\nvalue <- m${member}\nm${member}() + 1L\n"),
            &[],
        ));
    }
    for member in ["text", "`text`", "\"text\"", "`te\\x78t`"] {
        cases.push((
            format!("box::use(m = ./mod)\nm${member}() + 1L\n"),
            &["RY040"],
        ));
    }
    for member in ["label", "`label`", "\"la\\u0062el\""] {
        cases.push((
            format!("box::use(m = ./mod)\nm${member} + 1L\n"),
            &["RY040"],
        ));
    }
    for member in ["foo", "`foo`", "\"foo\""] {
        cases.push((
            format!("box::use(m = ./mod)\nm${member}(wrong = 1L)\n"),
            &["RY090"],
        ));
    }
    for member in ["`my-fn`", "\"my-\\u0066n\""] {
        cases.push((
            format!("box::use(m = ./renamed)\nvalue <- m${member}\nm${member}() + 1L\n"),
            &[],
        ));
    }
    cases.push((
        "box::use(m = ./renamed)\nm$`my-text`() + 1L\n".to_string(),
        &["RY040"],
    ));
    for (source, expected) in cases {
        let diagnostics = codes_for(root.path(), &source);
        assert_eq!(codes(&diagnostics), expected, "{source}: {diagnostics:#?}");
    }
    for member in ["missing", "`miss\\x69ng`", "\"missing\""] {
        for call in ["", "()"] {
            let source = format!("box::use(m = ./mod)\nm${member}{call}\n");
            let diagnostics = codes_for(root.path(), &source);
            assert_eq!(codes(&diagnostics), ["RY118"], "{source}: {diagnostics:#?}");
            assert_eq!(
                messages(&diagnostics, "RY118"),
                ["box module does not export `missing`"]
            );
        }
    }
}

#[test]
fn box_package_selective_import_uses_package_stub_without_attaching_package() {
    let source = "box::use(dplyr[filter])\nd <- data.frame(mpg = c(21, 22.8))\nfilter(d, mpg > 21)\nselect(d, mpg)\n";
    let mut checker = Checker::new("box-package.R");
    let diagnostics = findings(checker.check(&parse(Path::new("box-package.R"), source)));
    assert!(!diagnostics.iter().any(|(_, line, _)| *line == 2));
    assert!(lines(&diagnostics, "RY010").contains(&3));
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
    assert!(first[1].1.iter().any(|d| d.code == "RY118"));
    project.update_file(
        module.to_string_lossy().into_owned(),
        Arc::new(parse(&module, "foo <- 1L\nbar <- 2L\n")),
    );
    let second = project.check_incremental();
    assert!(!second[1].1.iter().any(|d| d.code == "RY118"));
    let cold = project.check();
    assert_eq!(second, cold, "warm importers must match a cold inventory");
    project.remove_file(&module.to_string_lossy());
    fs::remove_file(&module).unwrap();
    let missing = project.check_incremental();
    assert!(missing[0].1.iter().all(|d| d.code != "RY118"));
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
    let result = project.check();
    assert!(result[1].1.iter().any(|d| d.code == "RY118"));
    assert!(
        !module.exists(),
        "the editor overlay must not write the module"
    );
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
            diagnostics
                .iter()
                .all(|diagnostic| diagnostic.code != "RY040"),
            "reverse={reverse}: {diagnostics:#?}"
        );
    }
}

#[test]
fn oversized_module_is_opaque_instead_of_a_complete_inventory() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "large.r", vec![b' '; 1_048_577]);
    let diagnostics = codes_for(
        root.path(),
        "box::use(./large[missing])\nvalue <- missing\n",
    );
    assert!(
        !has(&diagnostics, "RY118") && !has(&diagnostics, "RY010"),
        "{diagnostics:#?}"
    );
}

#[test]
fn explicit_exports_override_tags_and_empty_calls_override_legacy_names() {
    let root = tempfile::tempdir().unwrap();
    write(
        root.path(),
        "override.r",
        "#' @export\nfoo <- function() 1L\nbar <- function() 2L\nbaz <- function() 3L\nbox::export(bar)\nbox::export(baz)\n",
    );
    write(
        root.path(),
        "empty.r",
        "foo <- function() 1L\nbox::export()\n",
    );
    let diagnostics = codes_for(
        root.path(),
        "box::use(./override[bar, baz, foo])\nbox::use(./empty[foo])\n",
    );
    assert_eq!(
        messages(&diagnostics, "RY118"),
        [
            "box module does not export `foo`",
            "box module does not export `foo`"
        ]
    );
    assert!(!has(&diagnostics, "RY010"));
}

#[test]
fn legacy_exports_exclude_dot_names_and_dynamic_exports_stay_uncertain() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "legacy.r", "foo <- 1L\n.hidden <- 2L\n");
    write(
        root.path(),
        "dynamic.r",
        "foo <- 1L\nbox::export(get('which_name'))\n",
    );
    let diagnostics = codes_for(
        root.path(),
        "box::use(./legacy[foo, .hidden])\nbox::use(./dynamic[missing])\n",
    );
    assert_eq!(
        messages(&diagnostics, "RY118"),
        ["box module does not export `.hidden`"]
    );
}

#[test]
fn selective_alias_and_wildcard_remove_the_renamed_original() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "mod.r", "foo <- 1L\nbar <- 2L\n");
    let diagnostics = codes_for(
        root.path(),
        "box::use(./mod[renamed = foo, ...])\nrenamed\nbar\nfoo\nmod\n",
    );
    assert_eq!(lines(&diagnostics, "RY010"), [3, 4]);
}

#[test]
fn nested_modules_use_the_callers_directory_and_prefer_lowercase_r() {
    let root = tempfile::tempdir().unwrap();
    let nested = root.path().join("nested");
    fs::create_dir_all(nested.join("mod")).unwrap();
    fs::create_dir_all(root.path().join("shared")).unwrap();
    write(&nested, "mod/hello.r", "lower <- 1L\n");
    write(&nested, "mod/hello.R", "upper <- 2L\n");
    write(root.path(), "shared/__init__.r", "parent <- 3L\n");
    let diagnostics = codes_for(
        &nested,
        "box::use(./mod/hello[lower, upper])\nbox::use(../shared[parent])\nlower\nparent\n",
    );
    assert_eq!(
        messages(&diagnostics, "RY118"),
        ["box module does not export `upper`"]
    );
}

#[test]
fn unresolved_modules_bind_named_imports_without_inventing_absence() {
    let root = tempfile::tempdir().unwrap();
    let diagnostics = codes_for(
        root.path(),
        "box::use(./missing[foo])\nfoo\nbox::use(./also_missing[...])\nother\n",
    );
    assert!(!has(&diagnostics, "RY118"));
    assert!(!has(&diagnostics, "RY010"));
}

#[test]
fn search_path_modules_bind_opaque_names_without_resolving_their_sources() {
    let root = tempfile::tempdir().unwrap();
    for source in [
        "box::use(mod/hello[answer])\nvalue <- answer\nother <- unbound\n",
        "box::use(mod/hello[renamed = answer])\nvalue <- renamed\nother <- unbound\n",
        "box::use(\"m\" = mod/hello)\nvalue <- m\nother <- unbound\n",
        "box::use(`\\x6d` = mod/hello)\nvalue <- m\nother <- unbound\n",
        "box::use(mod/hello)\nvalue <- hello\nother <- unbound\n",
    ] {
        let diagnostics = codes_for(root.path(), source);
        assert_eq!(codes(&diagnostics), ["RY010"], "{source}: {diagnostics:#?}");
        assert_eq!(
            messages(&diagnostics, "RY010"),
            ["variable `unbound` is not bound in this scope"]
        );
    }
    for source in [
        "box::use(mod/hello[...])\nvalue <- answer\nother <- unenumerated\n",
        "box::use(mod/hello[renamed = answer, ...])\nvalue <- renamed\nother <- unenumerated\n",
    ] {
        assert!(codes_for(root.path(), source).is_empty(), "{source}");
    }
    // A search-path module is not a relative path, even if the same suffix
    // happens to exist beside the caller. Its signatures remain opaque.
    fs::create_dir(root.path().join("mod")).unwrap();
    write(root.path(), "mod/hello.r", "answer <- function() 'wrong'\n");
    let diagnostics = codes_for(
        root.path(),
        "box::use(m = mod/hello[answer, missing])\nanswer() + 1L\nm$missing\n",
    );
    assert!(diagnostics.is_empty(), "{diagnostics:#?}");
}

#[test]
fn computed_target_retains_explicit_alias_as_opaque() {
    let root = tempfile::tempdir().unwrap();
    let diagnostics = codes_for(
        root.path(),
        "box::use(alias = make_path())\nvalue <- alias\n",
    );
    assert!(!has(&diagnostics, "RY010"), "{diagnostics:#?}");
}

#[test]
fn spaced_namespace_import_remains_visible_to_project_overlay() {
    let root = tempfile::tempdir().unwrap();
    let mut project = Project::new();
    add(&mut project, &root.path().join("mod.r"), "foo <- 1L\n");
    add(
        &mut project,
        &root.path().join("run.R"),
        "box :: use(./mod[foo, missing])\nfoo\n",
    );
    let diagnostics = project.check();
    assert!(
        diagnostics[1]
            .1
            .iter()
            .any(|diagnostic| diagnostic.code == "RY118")
    );
}

#[test]
fn package_aliases_and_objects_use_exact_package_signatures() {
    let source = "box::use(dplyr[other = filter])\nbox::use(dplyr)\nd <- data.frame(mpg = c(21, 22.8))\nother(d, mpg > 21)\ndplyr$filter(d, mpg > 21)\ndplyr$`filter`(d, mpg > 21)\ndplyr$\"filt\\u0065r\"(d, mpg > 21)\n";
    let mut checker = Checker::new("box-package-alias.R");
    let diagnostics = checker.check(&parse(Path::new("box-package-alias.R"), source));
    assert!(
        !diagnostics
            .iter()
            .any(|diagnostic| { diagnostic.code == "RY010" && diagnostic.message.contains("mpg") }),
        "{diagnostics:#?}"
    );
}

#[test]
fn local_function_signature_shadows_same_named_package_stub() {
    let root = tempfile::tempdir().unwrap();
    write(
        root.path(),
        "local.r",
        "filter <- function(data, predicate) { substitute(predicate); data }\n",
    );
    let diagnostics = codes_for(
        root.path(),
        "box::use(./local[filter])\nd <- data.frame(x = 1L)\nfilter(d, missing_column)\nfilter(d, wrong = x)\n",
    );
    assert!(
        !messages(&diagnostics, "RY010")
            .iter()
            .any(|message| message.contains("missing_column")),
        "{diagnostics:#?}"
    );
    // `wrong` is not a formal of the module function. The name-only
    // dplyr::filter signature must not be borrowed here.
    assert!(lines(&diagnostics, "RY090").contains(&3));
}

#[test]
fn function_local_import_does_not_escape_its_lexical_scope() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "mod.r", "foo <- function() 1L\n");
    let diagnostics = codes_for(
        root.path(),
        "run <- function() { box::use(./mod[foo]); foo() }\nrun()\nvalue <- foo\n",
    );
    assert_eq!(lines(&diagnostics, "RY010"), [2]);
}

#[test]
fn exported_returns_do_not_borrow_base_signatures_over_module_imports() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "a.r", "paste0 <- function(...) 1L\n");
    for (module, source) in [
        (
            "module_scope",
            "box::use(./a[paste0])\nfoo <- function(x = 1L) paste0('x')\nbox::export(foo)\n",
        ),
        (
            "function_scope",
            "foo <- function(x = 1L) { box::use(./a[paste0]); paste0('x') }\nbox::export(foo)\n",
        ),
    ] {
        write(root.path(), &format!("{module}.r"), source);
        for selection in ["[foo]", "[...]"] {
            let diagnostics = codes_for(
                root.path(),
                &format!(
                    "box::use(m = ./{module}{selection})\nfoo() + 1L\nm$foo() + 1L\nfoo(wrong = 1L)\nm$foo(wrong = 1L)\n"
                ),
            );
            assert!(
                !has(&diagnostics, "RY040"),
                "{module}{selection}: {diagnostics:#?}"
            );
            assert_eq!(
                lines(&diagnostics, "RY090").len(),
                2,
                "formals survive unknown returns: {diagnostics:#?}"
            );
        }
    }
    write(root.path(), "base.r", "foo <- function() paste0('x')\n");
    let control = codes_for(root.path(), "box::use(./base[foo])\nfoo() + 1L\n");
    assert!(has(&control, "RY040"));
}

#[test]
fn roxygen_regions_accept_comments_blank_lines_and_multiple_hashes() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "a.r", "foo <- 1L\n");
    for region in [
        "#' @export\n\n",
        "#' @export\n# ordinary comment\n",
        "##' @export\n",
        " \t###' \t@export\n",
    ] {
        for statement in ["foo <- 1L", "box::use(./a[foo])"] {
            write(
                root.path(),
                "tagged.r",
                format!(
                    "before <- function() {{\n  1L\n}}\n{region}{statement}\n#' @export\nbaz <- 2L\nafter <- 3L\n"
                ),
            );
            let diagnostics = codes_for(
                root.path(),
                "box::use(./tagged[foo, baz, before, after, absent])\n",
            );
            assert_eq!(
                messages(&diagnostics, "RY118"),
                [
                    "box module does not export `before`",
                    "box module does not export `after`",
                    "box module does not export `absent`"
                ],
                "{region:?} {statement}: {diagnostics:#?}"
            );
        }
    }
    // A tag inside the preceding function's source region cannot annotate
    // the following assignment, including two statements on the same line.
    for source in [
        "#' @export\nfoo <- function() {\n#' @export\n1L\n}\nbar <- 2L\n",
        "#' @export\nfoo <- 1L; bar <- 2L\n",
    ] {
        write(root.path(), "boundary.r", source);
        let diagnostics = codes_for(root.path(), "box::use(./boundary[bar])\n");
        assert!(has(&diagnostics, "RY118"), "{diagnostics:#?}");
    }
}

#[test]
fn box_123_does_not_recognize_export_tags_with_trailing_whitespace() {
    let root = tempfile::tempdir().unwrap();
    for suffix in [" ", "\t"] {
        write(
            root.path(),
            "legacy.r",
            format!("#' @export{suffix}\nfoo <- 1L\nbar <- 2L\n"),
        );
        let diagnostics = codes_for(root.path(), "box::use(./legacy[bar])\n");
        assert!(!has(&diagnostics, "RY118"), "{suffix:?}: {diagnostics:#?}");
    }
}

#[test]
fn explicit_reexports_keep_local_and_package_signatures() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "a.r", "foo <- function() 'wrong'\n");
    write(root.path(), "b.r", "box::use(./a[foo])\nbox::export(foo)\n");
    let diagnostics = codes_for(root.path(), "box::use(./b[foo])\nfoo() + 1L\n");
    assert!(has(&diagnostics, "RY040"));

    write(
        root.path(),
        "bridge.r",
        "box::use(dplyr[filter])\nbox::export(filter)\n",
    );
    let diagnostics = codes_for(
        root.path(),
        "box::use(./bridge)\nbox::use(./bridge[filter])\nd <- data.frame(mpg = c(21, 22.8))\nfilter(d, mpg > 21)\nbridge$filter(d, mpg > 21)\n",
    );
    assert!(!has(&diagnostics, "RY010"), "{diagnostics:#?}");
}

/// Write each `(module, source)` and import `foo` from it with `caller`.
fn each_module(root: &Path, modules: &[(&str, &str)], caller: &str) -> Vec<(String, Findings)> {
    modules
        .iter()
        .map(|(module, source)| {
            write(root, &format!("{module}.r"), source);
            let caller = caller.replace("{module}", module);
            (module.to_string(), codes_for(root, &caller))
        })
        .collect()
}

#[test]
fn legacy_absence_needs_proof_that_module_load_cannot_write_names() {
    let root = tempfile::tempdir().unwrap();
    let uncertain = [
        ("qualified", "base::assign('foo', function() 1L)\n"),
        ("source", "source('extra.R', local = TRUE)\n"),
        (
            "sys_source",
            "base::sys.source('extra.R', envir = environment())\n",
        ),
        ("load", "load('values.RData', envir = environment())\n"),
        ("computed", "initialize_module()\n"),
    ];
    for (module, diagnostics) in
        each_module(root.path(), &uncertain, "box::use(./{module}[foo])\nfoo\n")
    {
        assert!(!has(&diagnostics, "RY118"), "{module}: {diagnostics:#?}");
    }
    // A call in an uninvoked function body cannot create a load-time name.
    let proven = [
        ("inert", "setup <- function() base::assign('foo', 1L)\n"),
        ("known", "present <- 1L\n"),
    ];
    for (module, diagnostics) in each_module(root.path(), &proven, "box::use(./{module}[foo])\n") {
        assert!(has(&diagnostics, "RY118"), "{module}: {diagnostics:#?}");
    }
}

#[test]
fn exported_function_signature_requires_a_surviving_direct_definition() {
    let root = tempfile::tempdir().unwrap();
    let replaced = [
        (
            "wrapped",
            "foo <- function() 'old'\nfoo <- base::identity(function() 1L)\nbox::export(foo)\n",
        ),
        (
            "alias",
            "foo <- function() 'old'\nreplacement <- function() 1L\nfoo <- replacement\nbox::export(foo)\n",
        ),
        (
            "removed",
            "foo <- function() 'old'\nrm(foo)\nfoo <- base::identity(function() 1L)\nbox::export(foo)\n",
        ),
        (
            "direct_overwrite",
            "foo <- function() 'old'\nfoo <- function() 1L\nbox::export(foo)\n",
        ),
        (
            "nested",
            "foo <- function() 'old'\nif (TRUE) foo <- function() 1L\nbox::export(foo)\n",
        ),
        // Expression writes and runtime writers also invalidate the
        // imported callable's provenance.
        (
            "expression",
            "foo <- function() 'old'\ndummy <- (foo <- function() 1L)\nbox::export(foo)\n",
        ),
        (
            "runtime",
            "box::use(./a[foo])\nbase::assign('foo', function() 1L)\nbox::export(foo)\n",
        ),
    ];
    let surviving = [
        ("direct", "foo <- function() 'old'\nbox::export(foo)\n"),
        ("unmodified", "box::use(./a[foo])\nbox::export(foo)\n"),
    ];
    write(root.path(), "a.r", "foo <- function() 'old'\n");
    let caller = "box::use(./{module}[foo])\nfoo() + 1L\n";
    for (module, diagnostics) in each_module(root.path(), &replaced, caller) {
        assert!(!has(&diagnostics, "RY040"), "{module}: {diagnostics:#?}");
    }
    for (module, diagnostics) in each_module(root.path(), &surviving, caller) {
        assert!(has(&diagnostics, "RY040"), "{module}: {diagnostics:#?}");
    }
}

#[test]
fn legacy_inventory_includes_own_module_objects_but_not_attached_selections() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "a.r", "foo <- function() 1L\n");
    write(root.path(), "package.r", "box::use(dplyr)\nhelper <- 1L\n");
    write(root.path(), "local.r", "box::use(./a)\nhelper <- 1L\n");
    write(
        root.path(),
        "attached.r",
        "box::use(dplyr[filter])\nhelper <- 1L\n",
    );
    let diagnostics = codes_for(
        root.path(),
        "box::use(./package[dplyr, absent])\nbox::use(./local[a])\nbox::use(./attached[filter])\n",
    );
    assert_eq!(
        messages(&diagnostics, "RY118"),
        [
            "box module does not export `absent`",
            "box module does not export `filter`"
        ]
    );
    write(root.path(), "braced.r", "{{x <- 1L}}\n");
    let diagnostics = codes_for(root.path(), "box::use(./braced[x])\nx + 1L\n");
    assert!(!has(&diagnostics, "RY118"));
}

#[test]
fn roxygen_tagged_use_exports_object_and_attached_aliases() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "a.r", "foo <- function() 1L\n");
    write(
        root.path(),
        "tagged.r",
        "#' @export\nbox::use(imp = ./a[renamed = foo])\n#' @export\nlocal_value <- 1L\n",
    );
    write(
        root.path(),
        "attached.r",
        "#' @export\nbox::use(./a[renamed = foo])\n",
    );
    write(
        root.path(),
        "wildcard.r",
        "#' @export\nbox::use(./a[...])\n",
    );
    let diagnostics = codes_for(
        root.path(),
        "box::use(./tagged[imp, renamed, local_value, missing])\nbox::use(./attached[renamed])\nbox::use(./wildcard[foo, unknown])\n",
    );
    assert_eq!(
        messages(&diagnostics, "RY118"),
        ["box module does not export `missing`"],
        "{diagnostics:#?}"
    );
}

#[test]
fn quoted_box_aliases_use_runtime_binding_names_in_tagged_and_legacy_modules() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "a.r", "foo <- function() 1L\n");
    write(
        root.path(),
        "tagged.r",
        "#' @export\nbox::use(`ob\\x6a` = ./a[`ren\\x61med` = foo])\n",
    );
    write(root.path(), "legacy.r", "box::use(`ob\\x6a` = ./a)\n");
    write(
        root.path(),
        "strings.r",
        "#' @export\nbox::use(\"ob\\u006a\" = ./a[\"ren\\u0061med\" = foo])\n",
    );
    let diagnostics = codes_for(
        root.path(),
        "box::use(./tagged[obj, renamed])\nbox::use(./legacy[obj])\nbox::use(./strings[obj, renamed])\nobj$foo() + renamed()\n",
    );
    assert!(!has(&diagnostics, "RY118"), "{diagnostics:#?}");
}

#[test]
fn only_static_exact_export_tags_close_an_inventory() {
    let root = tempfile::tempdir().unwrap();
    let open = [
        // Only the exact `@export` tag closes a legacy inventory.
        (
            "s3",
            "#' @exportS3Method print foo\nfoo <- function() 1L\nbar <- 2L\n",
        ),
        // An aliased or computed box::export() keeps a tagged one open.
        (
            "alias",
            "#' @export\nfoo <- 1L\nbar <- 2L\nf <- box::export\nf(bar)\n",
        ),
        (
            "computed",
            "#' @export\nfoo <- 1L\nbar <- 2L\ndo.call(box::export, list(quote(bar)))\n",
        ),
    ];
    for (module, diagnostics) in each_module(root.path(), &open, "box::use(./{module}[bar])\nbar\n")
    {
        assert!(!has(&diagnostics, "RY118"), "{module}: {diagnostics:#?}");
    }
    let closed = [("tag_only", "#' @export\nfoo <- function() 1L\nbar <- 2L\n")];
    for (module, diagnostics) in each_module(root.path(), &closed, "box::use(./{module}[bar])\n") {
        assert!(has(&diagnostics, "RY118"), "{module}: {diagnostics:#?}");
    }
}

#[test]
fn legacy_expression_assignments_are_own_module_exports() {
    let root = tempfile::tempdir().unwrap();
    write(
        root.path(),
        "chain.r",
        "a <- b <- 1L\ndummy <- (foo <- 2L)\n",
    );
    let diagnostics = codes_for(root.path(), "box::use(./chain[a, b, foo])\na + b + foo\n");
    assert!(!has(&diagnostics, "RY118"), "{diagnostics:#?}");
    write(root.path(), "parent.r", "a <- b <<- 1L\n");
    let parent = codes_for(root.path(), "box::use(./parent[b])\n");
    assert!(
        has(&parent, "RY118"),
        "superassignment does not create an own-module export: {parent:#?}"
    );
}

#[test]
fn direct_superassignment_does_not_create_a_box_module_export() {
    let root = tempfile::tempdir().unwrap();
    let caller = "box::use(./{module}[foo])\n";
    let superassigned = [
        ("left", "foo <<- 1L\n"),
        ("right", "1L ->> foo\n"),
        ("parenthesized", "(foo <<- 1L)\n"),
        ("chained", "dummy <- (foo <<- 1L)\n"),
    ];
    for (module, diagnostics) in each_module(root.path(), &superassigned, caller) {
        assert!(has(&diagnostics, "RY118"), "{module}: {diagnostics:#?}");
    }
    let own = [
        ("outer", "foo <- (bar <<- 1L)\n"),
        ("same_outer", "foo <- (foo <<- 1L)\n"),
        ("existing", "foo <- 1L\nfoo <<- 2L\n"),
    ];
    for (module, diagnostics) in each_module(root.path(), &own, caller) {
        assert!(!has(&diagnostics, "RY118"), "{module}: {diagnostics:#?}");
    }
    // box errors while loading this module, so no selection from it can
    // provide a proven absence.
    write(
        root.path(),
        "tagged_super.r",
        "#' @export\nfoo <<- 1L\nbar <- 2L\n",
    );
    for selection in ["bar", "absent"] {
        let diagnostics = codes_for(
            root.path(),
            &format!("box::use(./tagged_super[{selection}])\n"),
        );
        assert!(!has(&diagnostics, "RY118"), "{selection}: {diagnostics:#?}");
    }
}

#[cfg(unix)]
#[test]
fn native_caller_and_nested_module_paths_do_not_use_lossy_parent() {
    use std::os::unix::ffi::OsStringExt;

    let root = tempfile::tempdir().unwrap();
    let raw_dir = root
        .path()
        .join(std::ffi::OsString::from_vec(b"raw\xff".to_vec()));
    let unicode_dir = root.path().join("raw�");
    fs::create_dir_all(&raw_dir).unwrap();
    fs::create_dir_all(&unicode_dir).unwrap();
    write(&raw_dir, "mod.r", "foo <- function() 1L\n");
    write(&unicode_dir, "mod.r", "foo <- function() 'wrong'\n");
    let caller = raw_dir.join("run.R");
    let mut file = parse(&caller, "box::use(./mod[foo])\nfoo() + 1L\n");
    file.native_path = Some(caller.clone());
    let mut checker = Checker::new(&caller.to_string_lossy());
    let diagnostics = findings(checker.check(&file));
    assert!(!has(&diagnostics, "RY040"), "{diagnostics:#?}");

    write(
        &raw_dir,
        "mod.r",
        "box::use(./inner[foo])\nbox::export(foo)\n",
    );
    write(&raw_dir, "inner.r", "foo <- function() 1L\n");
    write(&unicode_dir, "inner.r", "foo <- function() 'wrong'\n");
    let diagnostics = findings(checker.check(&file));
    assert!(!has(&diagnostics, "RY040"), "nested: {diagnostics:#?}");
}

#[test]
fn preferred_module_limits_do_not_select_a_different_file() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "mod.r", vec![b' '; 1_048_577]);
    write(root.path(), "mod.R", "foo <- function() 'wrong'\n");
    let diagnostics = codes_for(root.path(), "box::use(./mod[foo, missing])\nfoo() + 1L\n");
    assert!(
        !has(&diagnostics, "RY040") && !has(&diagnostics, "RY118"),
        "{diagnostics:#?}"
    );
    let caller = "box::use(./mod[foo])\nfoo() + 1L\n";
    fs::remove_file(root.path().join("mod.r")).unwrap();
    assert!(has(&codes_for(root.path(), caller), "RY040"));
    write(root.path(), "mod.r", [0xffu8]);
    assert!(!has(&codes_for(root.path(), caller), "RY040"));
}

#[test]
fn dotted_module_basename_keeps_its_full_name_before_suffix() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "foo.r", "foo <- function() 'wrong'\n");
    for module in ["foo.bar.r", "foo.bar.R"] {
        write(root.path(), module, "foo <- function() 1L\n");
        let diagnostics = codes_for(root.path(), "box::use(./foo.bar[foo])\nfoo() + 1L\n");
        assert!(!has(&diagnostics, "RY040"), "{module}: {diagnostics:#?}");
        fs::remove_file(root.path().join(module)).unwrap();
    }
}
