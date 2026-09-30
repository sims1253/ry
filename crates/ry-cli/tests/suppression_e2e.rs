//! CLI checks for native ignore lists at the comment parsing boundary.

use std::fs;
use std::process::Command;

#[test]
fn spaced_native_commas_validate_all_codes_and_keep_prose() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("example.R");
    let check = |source: &str| {
        fs::write(&path, source).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_ry"))
            .arg("check")
            .arg(&path)
            .output()
            .unwrap();
        String::from_utf8(output.stdout).unwrap()
    };

    for source in [
        "1L == NA # ry: ignore RY034 , RX034\n",
        "1L == NA # ry: ignore RY034 ,RX034\n",
        "# ry: ignore RY034 , RX034\n1L == NA\n",
    ] {
        let output = check(source);
        assert!(output.contains("[RY034]"), "{source}: {output}");
        assert!(output.contains("[RY112]"), "{source}: {output}");
    }

    for source in [
        "list(\"a\" <- (1L == NA)) # ry: ignore RY034 , RY102\n",
        "# ry: ignore RY034 ,RY102\nlist(\"a\" <- (1L == NA))\n",
        "1L == NA # ry: ignore RY034 reason documented under RY102\n",
        "1L == NA # ry: ignore[RY034] reason documented under RY102\n",
    ] {
        let output = check(source);
        assert!(!output.contains("[RY034]"), "{source}: {output}");
        assert!(!output.contains("[RY102]"), "{source}: {output}");
        assert!(!output.contains("[RY112]"), "{source}: {output}");
    }

    let output = check("1L == NA # ry: ignore: RY034 reason\n");
    assert!(output.contains("[RY034]"), "{output}");
    assert!(output.contains("[RY112]"), "{output}");
}
