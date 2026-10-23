//! Files ry cannot parse at all still reach every output format (#743).

use crate::support::{check, write};

/// One call nested past ry's syntax nesting limit of 128.
fn too_deep() -> Vec<u8> {
    format!("{}1{}\n", "f(".repeat(512), ")".repeat(512)).into_bytes()
}

#[test]
fn unparseable_file_is_one_ry000_in_every_format() {
    let temp = tempfile::tempdir().unwrap();
    let deep = too_deep();
    write(temp.path(), &[("deep.R", &deep), ("ok.R", b"x <- 1\n")]);

    let json = check()
        .cwd(temp.path())
        .args(["--output-format", "json", "--exit-zero", "deep.R", "ok.R"])
        .run();
    assert!(json.success(), "{json:?}");
    assert_eq!(json.findings(), [["deep.R", "RY000", "error"]]);
    let diagnostic = &json.json()[0];
    assert_eq!(
        (&diagnostic["line"], &diagnostic["column"]),
        (&1.into(), &1495.into())
    );
    assert!(
        diagnostic["message"]
            .as_str()
            .unwrap()
            .contains("syntax nesting limit of 128"),
        "{diagnostic}"
    );

    for (format, expected) in [
        ("concise", "deep.R:1:1495: error: [RY000]"),
        ("full", "deep.R:1:1495: error: [RY000]"),
        ("github", "::error file=deep.R,line=1,col=1495::RY000:"),
        ("gitlab", "\"check_name\": \"RY000\""),
        ("junit", "<testcase name=\"RY000: deep.R:1\""),
    ] {
        let output = check()
            .cwd(temp.path())
            .args(["--output-format", format, "deep.R"])
            .run();
        assert_eq!(output.code(), Some(1), "{format}: {output:?}");
        assert!(output.stdout().contains(expected), "{format}: {output:?}");
    }

    let human = check().cwd(temp.path()).arg("deep.R").run();
    assert!(
        human.stderr().contains("checked 0 file(s), 1 error(s)"),
        "{human:?}"
    );
}

/// Invalid UTF-8 decodes lossily, so an unreadable file needs a permission
/// error. Root reads it anyway, so the test skips itself there.
#[cfg(unix)]
#[test]
fn unreadable_file_is_one_ry000_at_the_start() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::tempdir().unwrap();
    write(temp.path(), &[("locked.R", b"x <- 1\n")]);
    let locked = temp.path().join("locked.R");
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
    if std::fs::read(&locked).is_ok() {
        eprintln!("skipping: permissions do not stop this user from reading");
        return;
    }

    let json = check()
        .cwd(temp.path())
        .args(["--output-format", "json", "locked.R"])
        .run();
    assert_eq!(json.code(), Some(1), "{json:?}");
    assert_eq!(json.findings(), [["locked.R", "RY000", "error"]]);
    let diagnostic = &json.json()[0];
    assert_eq!(
        (&diagnostic["line"], &diagnostic["column"]),
        (&1.into(), &1.into())
    );
    assert_eq!(
        diagnostic["message"],
        "cannot read file: Permission denied (os error 13); file not checked"
    );
}
