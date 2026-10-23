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
