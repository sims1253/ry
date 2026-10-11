//! CLI check for native ignore lists at the comment parsing boundary. The
//! comment grammar itself is unit-tested in the checker.

use ry_testkit::ry;

#[test]
fn colon_after_native_ignore_is_not_a_suppression() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(
        tmp.path().join("example.R"),
        "1L == NA # ry: ignore: RY034 reason\n",
    )
    .unwrap();
    let output = ry!()
        .cwd(tmp.path())
        .args(["check", "example.R", "--output-format", "json"])
        .run();
    let mut codes = output.codes();
    codes.sort();
    assert_eq!(codes, ["RY034", "RY112"], "{output:?}");
}
