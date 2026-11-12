//! `ry reduce` shrinks a file while it keeps the target diagnostic.

use std::process::Command;

use ry_testkit::{CliOutput, FixtureProject, ry};

const SOURCE: &str = "\
# Helpers the diagnostic does not need.
helper <- function(a, b) {
  total <- a + b
  total * 2
}
x <- 1
w <- helper(1, 2)
if (w > 0) {
  z <- \"a\" + x
}
print(w)
";

fn reduce(project: &FixtureProject, args: &[&str]) -> CliOutput {
    ry!()
        .cwd(project.root())
        .arg("reduce")
        .args(args)
        .env("RY_NO_INSTALLED_LIBRARIES", "1")
        .run()
}

/// Rule codes `ry check` reports for `source` saved as `name`.
fn codes(project: &FixtureProject, name: &str, source: &str) -> Vec<String> {
    project.write_file(name, source).unwrap();
    let output = ry!()
        .cwd(project.root())
        .args(["check", "--output-format", "json", "--exit-zero", name])
        .env("RY_NO_INSTALLED_LIBRARIES", "1")
        .run();
    assert!(output.success(), "{output:?}");
    output.codes()
}

#[test]
fn reduce_keeps_only_what_the_diagnostic_needs() {
    let project = FixtureProject::empty().unwrap();
    project.write_file("bug.R", SOURCE).unwrap();
    let output = reduce(&project, &["bug.R", "--code", "RY040"]);
    assert!(output.success(), "{output:?}");
    let reduced = output.stdout();
    assert_eq!(reduced, "x <- 1\n\"a\" + x\n", "{output:?}");
    assert_eq!(codes(&project, "reduced.R", &reduced), ["RY040"]);
    // The file on disk is unchanged.
    let on_disk = std::fs::read_to_string(project.path("bug.R")).unwrap();
    assert_eq!(on_disk, SOURCE);
}

#[test]
fn reduce_selects_the_diagnostic_on_a_line() {
    let project = FixtureProject::empty().unwrap();
    project
        .write_file("bug.R", "a <- \"x\" + 1\nb <- 2\nc <- TRUE + \"y\"\n")
        .unwrap();
    let output = reduce(&project, &["bug.R", "--code", "RY040", "--line", "3"]);
    assert!(output.success(), "{output:?}");
    assert_eq!(output.stdout(), "TRUE + \"y\"\n", "{output:?}");
}

#[test]
fn reduce_fails_without_the_target_diagnostic() {
    let project = FixtureProject::empty().unwrap();
    project.write_file("ok.R", "x <- 1\n").unwrap();
    let output = reduce(&project, &["ok.R", "--code", "RY040"]);
    assert_eq!(output.code(), Some(1), "{output:?}");
    assert!(output.stderr().contains("reports no RY040"), "{output:?}");
    assert!(output.stdout().is_empty(), "{output:?}");
}

#[test]
fn reduce_with_rscript_keeps_the_code_r_needs_to_run() {
    if Command::new("Rscript").arg("--version").output().is_err() {
        eprintln!("SKIP: Rscript is not on PATH");
        return;
    }
    let project = FixtureProject::empty().unwrap();
    let source = "x <- c(1, NA)\nunused <- 3\ny <- x == NA\nprint(y)\n";
    project.write_file("na.R", source).unwrap();

    let plain = reduce(&project, &["na.R", "--code", "RY034"]);
    assert!(plain.success(), "{plain:?}");
    assert_eq!(plain.stdout(), "x == NA\n", "{plain:?}");

    let output = reduce(&project, &["na.R", "--code", "RY034", "--rscript"]);
    assert!(output.success(), "{output:?}");
    // R needs a binding for `x`, but not its values.
    assert_eq!(output.stdout(), "x <- c()\nx == NA\n", "{output:?}");
}
