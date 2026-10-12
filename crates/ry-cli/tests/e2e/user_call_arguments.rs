//! `ry check` on a multi-file project validates calls to the project's own
//! functions when every definition of the callee agrees on its formals.
use ry_testkit::FixtureProject;
use serde_json::Value;
use std::process::Command;

const HELPERS: &str = "\
helper <- function(x, scale) x * scale
lazy <- function(x, unused) x
gather <- function(x, ...) list(x, ...)
twice <- function(a) a
memo <- function(a) a
pick <- function(x, i) x[[i]]
`pick<-` <- function(x, i, strict = FALSE, value) {
  x[[i]] <- value
  x
}
scaled <- helper(2)
alias <- function(x) x
";

const OTHER_HELPERS: &str = "\
twice <- function(a, b) a + b
memo <- Vectorize(memo)
`alias` <- function(y) y
";

const CALLS: &str = "\
use_helpers <- function(values) {
  helper(1, 2, sclae = 3)
  helper(1)
  lazy(1)
  gather(1, anything = 2)
  twice(1, c = 3)
  twice()
  memo(b = 1)
  alias(y = 1)
  pick(values, 1, strict = TRUE) <- 2
  values
}
apply_helper <- function(helper) helper()
choose_helper <- function(flag) {
  helper <- if (flag) lazy else twice
  helper(1)
}
qualified <- function() other::helper(1)
test_block <- suppressWarnings({
  helper <- function(y) y
  helper(y = 1)
})
";

fn diagnostics() -> Vec<Value> {
    let files = FixtureProject::empty().unwrap();
    files
        .write_file("DESCRIPTION", "Package: argcheck\nVersion: 0.1.0\n")
        .unwrap();
    files.write_file("NAMESPACE", "").unwrap();
    files.write_file("R/helpers.R", HELPERS).unwrap();
    files.write_file("R/more_helpers.R", OTHER_HELPERS).unwrap();
    files.write_file("R/calls.R", CALLS).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_ry"))
        .args(["check", "--output-format", "json", "--exit-zero"])
        .arg(files.root())
        .env("RY_NO_INSTALLED_LIBRARIES", "1")
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| panic!("{error}: {output:?}"))
}

#[test]
fn project_calls_are_checked_against_unambiguous_definitions() {
    let diagnostics = diagnostics();
    let mut found: Vec<String> = diagnostics
        .iter()
        .filter(|diagnostic| matches!(diagnostic["code"].as_str(), Some("RY090" | "RY091")))
        .map(|diagnostic| {
            let path = diagnostic["path"].as_str().unwrap();
            format!(
                "{}:{} {} {}",
                path.rsplit('/').next().unwrap(),
                diagnostic["line"],
                diagnostic["code"].as_str().unwrap(),
                diagnostic["message"].as_str().unwrap(),
            )
        })
        .collect();
    found.sort();
    // `lazy(1)` never forces `unused`, `gather` absorbs names in `...`,
    // `twice` and `alias` (another spelling of the same R name) have two
    // definitions with different formals, `memo` is rebound
    // to a non-literal, a replacement target calls `pick<-` rather than
    // `pick`, and a formal, local rebinding, block-local literal, or another
    // package's export named `helper` is not the project function.
    assert_eq!(
        found,
        [
            "calls.R:2 RY090 unknown argument `sclae` to `helper`; did you mean `scale`?",
            "calls.R:3 RY091 missing required argument `scale` in call to `helper`",
            "helpers.R:11 RY091 missing required argument `scale` in call to `helper`",
        ],
        "{diagnostics:#?}"
    );
}
