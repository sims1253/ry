//! GNU R as the referee for `ry dump-facts` claims (#858).
//!
//! A fixture in `crates/ry-checker/testdata/oracle/` opts in with one or
//! more `# oracle-facts: name ...` lines naming top-level bindings. For each
//! such fixture this harness exports the file with the real `ry dump-facts`,
//! runs the same file in `Rscript --vanilla` with a read-only probe appended,
//! and fails when R contradicts a concrete claim in the export:
//!
//! - `mode` against `typeof()`. A union allows each member's mode; `opaque`,
//!   or a union with an opaque or unknown member, claims nothing.
//! - A `known`, `nonempty`, or `at_least` `length` against `length()`.
//! - A `known` `class` against `oldClass()`, where `NULL` is the empty list
//!   and a possibly truncated list must be a prefix.
//!
//! A declared binding that ry exports without any of these claims also fails,
//! so a fixture cannot pass by losing precision.
//!
//! The probe sees only the global environment after the file has run, so
//! function locals cannot be declared; bind a call's result at top level.
//! Like the full checker oracle matrix, the R-backed tests are `#[ignore]`d
//! (each debug `dump-facts` run hashes its own binary, which takes seconds).
//! Run them with
//! `cargo test -p ry-cli --test e2e facts_oracle -- --include-ignored`.
//! They skip cleanly when `Rscript` is not installed; CI's `Oracle (R)` job
//! installs R and runs them.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use ry_testkit::ry;
use serde_json::{Value, json};

const PROBE_TAG: &str = "ry-oracle-fact";

fn oracle_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../ry-checker/testdata/oracle")
}

fn rscript_available() -> bool {
    Command::new("Rscript")
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success())
}

/// Binding names declared by `# oracle-facts:` lines, in order.
fn declared_bindings(src: &str) -> Result<Vec<String>, String> {
    let prefix = "oracle-facts:";
    let mut names: Vec<String> = Vec::new();
    for line in src.lines() {
        let trimmed = line
            .trim_start_matches([' ', '\t'])
            .trim_start_matches('#')
            .trim();
        if !trimmed.to_ascii_lowercase().starts_with(prefix) {
            continue;
        }
        let rest = &trimmed[prefix.len()..];
        if rest.trim().is_empty() {
            return Err("`# oracle-facts:` names no binding".into());
        }
        for name in rest.split_whitespace() {
            // The probe embeds each name in an R string literal.
            let plain = name
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic() || c == '.')
                && name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_');
            if !plain {
                return Err(format!(
                    "`# oracle-facts:` name {name:?} is not a plain R name"
                ));
            }
            if names.iter().any(|seen| seen == name) {
                return Err(format!("duplicate oracle fact {name}"));
            }
            names.push(name.to_owned());
        }
    }
    Ok(names)
}

/// What GNU R reports for one binding after the fixture has run.
#[derive(Debug)]
struct Observed {
    type_of: String,
    length: u64,
    /// `oldClass()`; `NULL` is empty.
    class: Vec<String>,
}

/// Read-only R code appended to the fixture. Every call is `base::`
/// qualified so fixture bindings cannot shadow the probe.
fn probe(names: &[String]) -> String {
    let quoted: Vec<String> = names.iter().map(|name| format!("\"{name}\"")).collect();
    format!(
        r#"
base::local({{
  for (name in base::c({names})) {{
    value <- base::get(name, envir = base::globalenv(), inherits = FALSE)
    class <- base::oldClass(value)
    base::cat("{PROBE_TAG}", name, base::typeof(value),
      base::format(base::length(value), scientific = FALSE),
      base::length(class), class, sep = "\t")
    base::cat("\n")
  }}
}})
"#,
        names = quoted.join(", ")
    )
}

fn parse_observations(stdout: &str) -> Result<HashMap<String, Observed>, String> {
    let mut observed = HashMap::new();
    for line in stdout.lines() {
        let mut fields = line.split('\t');
        if fields.next() != Some(PROBE_TAG) {
            continue;
        }
        let malformed = || format!("malformed probe line {line:?}");
        let name = fields.next().ok_or_else(malformed)?;
        let type_of = fields.next().ok_or_else(malformed)?;
        let length = fields.next().and_then(|field| field.parse().ok());
        let count: Option<usize> = fields.next().and_then(|field| field.parse().ok());
        let class: Vec<String> = fields.map(str::to_owned).collect();
        let (Some(length), Some(count)) = (length, count) else {
            return Err(malformed());
        };
        if count != class.len() {
            return Err(malformed());
        }
        observed.insert(
            name.to_owned(),
            Observed {
                type_of: type_of.to_owned(),
                length,
                class,
            },
        );
    }
    Ok(observed)
}

/// R `typeof()` values a structured type's `mode` allows, or `None` when the
/// mode makes no claim.
fn allowed_typeofs(ty: &Value) -> Option<Vec<&'static str>> {
    let modes: &[&str] = match ty["mode"].as_str()? {
        "logical" => &["logical"],
        "integer" => &["integer"],
        "double" => &["double"],
        "complex" => &["complex"],
        "character" => &["character"],
        "raw" => &["raw"],
        "list" => &["list"],
        "null" => &["NULL"],
        "function" => &["closure", "builtin", "special"],
        "environment" => &["environment"],
        "language" => &["symbol", "language"],
        "union" => {
            if ty["members"]["kind"] != "known" {
                return None;
            }
            let mut allowed = Vec::new();
            for member in ty["members"]["types"].as_array()? {
                for mode in allowed_typeofs(member)? {
                    if !allowed.contains(&mode) {
                        allowed.push(mode);
                    }
                }
            }
            return (!allowed.is_empty()).then_some(allowed);
        }
        // `opaque`, and any value a later schema adds, claims nothing.
        _ => return None,
    };
    Some(modes.to_vec())
}

/// Every claim R contradicts for one binding, or a note that the export
/// makes no claim the oracle can check.
fn contradictions(name: &str, ty: &Value, observed: &Observed) -> Vec<String> {
    let mut failures = Vec::new();
    let mut claims = 0;
    if let Some(allowed) = allowed_typeofs(ty) {
        claims += 1;
        if !allowed.contains(&observed.type_of.as_str()) {
            failures.push(format!(
                "{name}: ry mode {} allows typeof {allowed:?}, R says {:?}",
                ty["mode"], observed.type_of
            ));
        }
    }
    let length = &ty["length"];
    let bound = match length["kind"].as_str() {
        Some("known") => length["value"].as_u64().map(|value| (value, true)),
        Some("nonempty") => Some((1, false)),
        Some("at_least") => length["value"].as_u64().map(|value| (value, false)),
        _ => None,
    };
    if let Some((value, exact)) = bound {
        claims += 1;
        let holds = if exact {
            observed.length == value
        } else {
            observed.length >= value
        };
        if !holds {
            failures.push(format!(
                "{name}: ry length {length} but R length() is {}",
                observed.length
            ));
        }
    }
    let class = &ty["class"];
    if class["kind"] == "known"
        && let Some(names) = class["names"].as_array()
    {
        claims += 1;
        let prefix_only = class["may_be_truncated"] == true;
        let size_ok = if prefix_only {
            observed.class.len() >= names.len()
        } else {
            observed.class.len() == names.len()
        };
        // A null name is a position whose class the checker does not know.
        let names_ok = names
            .iter()
            .zip(&observed.class)
            .all(|(claimed, actual)| claimed.as_str().is_none_or(|claimed| claimed == actual));
        if !(size_ok && names_ok) {
            failures.push(format!(
                "{name}: ry class {class} but R oldClass() is {:?}",
                observed.class
            ));
        }
    }
    if claims == 0 {
        failures.push(format!(
            "{name}: ry exports no concrete mode, length, or class claim to check"
        ));
    }
    failures
}

/// Export `src` with `ry dump-facts`, observe it in R, and return every
/// failure. `tamper` may edit each declared binding's exported type before
/// comparison; the oracle suite passes a no-op.
fn check_fixture(
    fixture: &str,
    src: &str,
    names: &[String],
    tamper: &dyn Fn(&str, &mut Value),
) -> Vec<String> {
    let scratch = tempfile::tempdir().expect("temp dir");
    let ry_dir = scratch.path().join("ry");
    let r_dir = scratch.path().join("r");
    fs::create_dir_all(&ry_dir).unwrap();
    fs::create_dir_all(&r_dir).unwrap();
    fs::write(ry_dir.join(fixture), src).unwrap();
    fs::write(r_dir.join(fixture), format!("{src}\n{}", probe(names))).unwrap();

    let export = ry!()
        .cwd(&ry_dir)
        .env("RY_NO_INSTALLED_LIBRARIES", "1")
        .arg("dump-facts")
        .arg(fixture)
        .run();
    if !export.success() {
        return vec![format!(
            "{fixture}: ry dump-facts failed: {}",
            export.stderr()
        )];
    }
    let facts = export.json();

    let run = Command::new("Rscript")
        .current_dir(&r_dir)
        .arg("--vanilla")
        .arg(fixture)
        .output()
        .expect("spawn Rscript");
    if !run.status.success() {
        return vec![format!(
            "{fixture}: Rscript --vanilla failed: {}",
            String::from_utf8_lossy(&run.stderr).trim()
        )];
    }
    let observed = match parse_observations(&String::from_utf8_lossy(&run.stdout)) {
        Ok(observed) => observed,
        Err(error) => return vec![format!("{fixture}: {error}")],
    };

    let top = facts["files"][0]["scopes"]
        .as_array()
        .and_then(|scopes| scopes.iter().find(|scope| scope["kind"] == "top"));
    let mut failures = Vec::new();
    for name in names {
        let ty = top
            .and_then(|scope| scope["bindings"].as_array())
            .and_then(|bindings| bindings.iter().find(|binding| binding["name"] == **name))
            .map(|binding| binding["type"].clone());
        let Some(mut ty) = ty else {
            failures.push(format!("{fixture}: ry exports no top-level binding {name}"));
            continue;
        };
        let Some(observed) = observed.get(name) else {
            failures.push(format!("{fixture}: the R probe did not report {name}"));
            continue;
        };
        tamper(name, &mut ty);
        failures.extend(
            contradictions(name, &ty, observed)
                .into_iter()
                .map(|failure| format!("{fixture}: {failure}")),
        );
    }
    failures
}

/// `(file name, source, declared bindings)` for every opted-in fixture.
fn fact_fixtures() -> Vec<(String, String, Vec<String>)> {
    let mut paths: Vec<PathBuf> = fs::read_dir(oracle_dir())
        .expect("read oracle dir")
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "R"))
        .collect();
    paths.sort();
    let mut fixtures = Vec::new();
    for path in paths {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let src = fs::read_to_string(&path).expect("read fixture");
        let names = declared_bindings(&src).unwrap_or_else(|error| panic!("{name}: {error}"));
        if !names.is_empty() {
            fixtures.push((name, src, names));
        }
    }
    fixtures
}

#[test]
#[ignore]
fn oracle_fact_claims_agree_with_r() {
    if !rscript_available() {
        eprintln!("Rscript not on PATH; skipping the dump-facts oracle.");
        return;
    }
    let fixtures = fact_fixtures();
    assert!(!fixtures.is_empty(), "no `# oracle-facts:` fixtures found");
    let failures: Vec<String> = fixtures
        .iter()
        .flat_map(|(fixture, src, names)| check_fixture(fixture, src, names, &|_, _| {}))
        .collect();
    assert!(
        failures.is_empty(),
        "R contradicts {} dump-facts claim(s):\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// The full pipeline on a real fixture, with one exported claim made wrong
/// on purpose: R must catch it.
#[test]
#[ignore]
fn oracle_fails_a_deliberately_wrong_claim() {
    if !rscript_available() {
        eprintln!("Rscript not on PATH; skipping the dump-facts oracle.");
        return;
    }
    let (fixture, src, names) = fact_fixtures()
        .into_iter()
        .find(|(fixture, ..)| fixture == "facts_atomic_vectors.R")
        .expect("facts_atomic_vectors.R declares facts");
    let failures = check_fixture(&fixture, &src, &names, &|name, ty| {
        if name == "int" {
            ty["mode"] = json!("double");
        }
    });
    // Exactly one failure: every untampered claim in the fixture holds.
    assert_eq!(failures.len(), 1, "{failures:#?}");
    assert!(
        failures[0].contains("int: ry mode \"double\"") && failures[0].contains("\"integer\""),
        "{failures:#?}"
    );
}

fn known(value: u64) -> Value {
    json!({"kind": "known", "value": value})
}

fn class(names: &[&str], truncated: bool) -> Value {
    json!({"kind": "known", "names": names, "capacity": 4, "may_be_truncated": truncated})
}

fn r_type(mode: &str, length: Value, class: Value) -> Value {
    json!({"kind": "r_type", "mode": mode, "length": length, "class": class,
        "members": {"kind": "not_applicable"}})
}

fn observed(type_of: &str, length: u64, class: &[&str]) -> Observed {
    Observed {
        type_of: type_of.to_owned(),
        length,
        class: class.iter().map(|name| (*name).to_owned()).collect(),
    }
}

#[test]
fn each_claim_kind_is_checked() {
    let unknown_class = json!({"kind": "unknown", "names": []});
    let unknown_length = json!({"kind": "unknown"});
    let int = observed("integer", 3, &[]);
    // Agreement, including a lower bound and the builtin/closure split.
    assert!(contradictions("x", &r_type("integer", known(3), class(&[], false)), &int).is_empty());
    assert!(
        contradictions(
            "x",
            &r_type(
                "integer",
                json!({"kind": "at_least", "value": 2}),
                class(&[], false)
            ),
            &int
        )
        .is_empty()
    );
    assert!(
        contradictions(
            "f",
            &r_type("function", known(1), unknown_class.clone()),
            &observed("builtin", 1, &[])
        )
        .is_empty()
    );
    // Each claim contradicted on its own.
    let empty = observed("integer", 0, &[]);
    for (ty, r, claim) in [
        (
            r_type("double", unknown_length.clone(), unknown_class.clone()),
            &int,
            "mode",
        ),
        (
            r_type("opaque", known(2), unknown_class.clone()),
            &int,
            "length",
        ),
        (
            r_type(
                "opaque",
                json!({"kind": "at_least", "value": 4}),
                unknown_class.clone(),
            ),
            &int,
            "length",
        ),
        (
            r_type("opaque", json!({"kind": "nonempty"}), unknown_class.clone()),
            &empty,
            "length",
        ),
        (
            r_type("opaque", unknown_length.clone(), class(&["factor"], false)),
            &int,
            "class",
        ),
    ] {
        let failures = contradictions("x", &ty, r);
        assert_eq!(failures.len(), 1, "{ty}: {failures:?}");
        assert!(
            failures[0].starts_with(&format!("x: ry {claim}")),
            "{failures:?}"
        );
    }
    // A union allows its members' modes, and nothing else.
    let mut union = r_type("union", known(1), class(&[], false));
    union["members"] = json!({"kind": "known", "types": [
        r_type("character", known(1), class(&[], false)),
        r_type("integer", known(1), class(&[], false)),
    ]});
    assert!(contradictions("u", &union, &observed("integer", 1, &[])).is_empty());
    assert_eq!(
        contradictions("u", &union, &observed("double", 1, &[])).len(),
        1
    );
    // An opaque member removes the mode claim.
    union["members"]["types"][1] = r_type("opaque", unknown_length.clone(), unknown_class.clone());
    assert!(contradictions("u", &union, &observed("double", 1, &[])).is_empty());
    // A possibly truncated class must be a prefix of R's.
    let five = observed("integer", 1, &["a", "b", "c", "d", "e"]);
    let truncated = r_type(
        "opaque",
        unknown_length.clone(),
        class(&["a", "b", "c", "d"], true),
    );
    assert!(contradictions("x", &truncated, &five).is_empty());
    let exact = r_type(
        "opaque",
        unknown_length.clone(),
        class(&["a", "b", "c", "d"], false),
    );
    assert_eq!(contradictions("x", &exact, &five).len(), 1);
    // A binding without any claim cannot pass silently.
    let failures = contradictions("x", &r_type("opaque", unknown_length, unknown_class), &int);
    assert_eq!(failures.len(), 1);
    assert!(failures[0].contains("no concrete"), "{failures:?}");
}

#[test]
fn declarations_reject_unsafe_or_duplicate_names() {
    assert_eq!(
        declared_bindings("# oracle: must-pass\n# oracle-facts: a b.c\n#oracle-facts: .d_e\n"),
        Ok(vec!["a".into(), "b.c".into(), ".d_e".into()])
    );
    assert!(declared_bindings("# oracle-facts:\n").is_err());
    assert!(declared_bindings("# oracle-facts: a a\n").is_err());
    assert!(declared_bindings("# oracle-facts: `a b`\n").is_err());
    assert!(declared_bindings("# oracle-facts: a\")\n").is_err());
}
