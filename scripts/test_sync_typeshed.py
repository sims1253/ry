import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


class SyncTypeshedTest(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.repo = self.root / "ry checkout"
        scripts = self.repo / "scripts"
        scripts.mkdir(parents=True)
        self.script = scripts / "sync_typeshed.sh"
        shutil.copyfile(Path(__file__).with_name("sync_typeshed.sh"), self.script)
        self.vendor = self.repo / "crates/ry-typeshed/vendor"
        self.vendor.mkdir(parents=True)
        (self.vendor / "old.json").write_text("old snapshot")
        (self.vendor / "SOURCE").write_text("old provenance")
        self.checkout = self.root / "typeshed checkout"
        stubs = self.checkout / "stubs/base"
        stubs.mkdir(parents=True)
        (stubs / "base.json").write_text('{"package": "base"}\n')
        binaries = self.root / "bin"
        self.binaries = binaries
        binaries.mkdir()
        cargo = binaries / "cargo"
        cargo.write_text("""#!/usr/bin/env bash
set -eu
candidate=${!#}
test -f "$candidate/base/base.json"
test -f "$candidate/SOURCE"
# The old snapshot must remain available throughout validation.
test "$(cat "$EXPECTED_VENDOR/old.json")" = "old snapshot"
if [[ -n "${FRESHNESS_MANIFEST:-}" ]]; then
  "$REAL_CARGO" build --offline --quiet --manifest-path "$FRESHNESS_MANIFEST"
fi
exit "$VALIDATION_STATUS"
""")
        cargo.chmod(0o755)
        self.env = dict(os.environ, PATH=f"{binaries}:{os.environ['PATH']}",
                        EXPECTED_VENDOR=str(self.vendor))

    def run_sync(self, status):
        return subprocess.run(
            ["bash", str(self.script), str(self.checkout)],
            env=dict(self.env, VALIDATION_STATUS=str(status)),
            capture_output=True, text=True,
        )

    def assert_no_staging_directory(self):
        self.assertEqual(list(self.vendor.parent.glob("vendor.*")), [])

    def assert_old_snapshot(self, path=None):
        path = self.vendor if path is None else path
        self.assertEqual((path / "old.json").read_text(), "old snapshot")
        self.assertEqual((path / "SOURCE").read_text(), "old provenance")
        self.assertEqual(sorted(p.name for p in path.iterdir()),
                         ["SOURCE", "old.json"])

    def fail_move(self, mode):
        move = self.binaries / "mv"
        move.write_text("""#!/usr/bin/env bash
set -eu
if [[ "$MOVE_FAILURE" == backup && "$1" == "$EXPECTED_VENDOR" ]]; then
  exit 7
fi
if [[ "$2" == "$EXPECTED_VENDOR" ]]; then
  if [[ "${1##*/}" == snapshot ]]; then
    [[ "$MOVE_FAILURE" != restore ]] || exit 9
  elif [[ "$MOVE_FAILURE" == interrupt ]]; then
    "$REAL_MV" "$@"
    kill -TERM "$PPID"
    exit 0
  else
    exit 7
  fi
fi
exec "$REAL_MV" "$@"
""")
        move.chmod(0o755)
        self.env.update(MOVE_FAILURE=mode, REAL_MV=shutil.which("mv"))

    def test_failed_validation_preserves_snapshot_and_provenance(self):
        result = self.run_sync(7)
        self.assertEqual(result.returncode, 7, result.stderr)
        self.assert_old_snapshot()
        self.assert_no_staging_directory()

    def test_success_replaces_snapshot_after_validation(self):
        result = self.run_sync(0)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse((self.vendor / "old.json").exists())
        self.assertEqual((self.vendor / "base/base.json").read_bytes(),
                         (self.checkout / "stubs/base/base.json").read_bytes())
        self.assertIn("stubs-sha256:", (self.vendor / "SOURCE").read_text())
        self.assert_no_staging_directory()

    @unittest.skipUnless(shutil.which("cargo"), "requires Cargo")
    def test_next_cargo_build_embeds_the_installed_snapshot(self):
        # The validator builds after staging, while the original vendor files
        # are still installed. Cargo must not consider the replacement fresh
        # merely because its copied mtimes predate that build.
        manifest = self.repo / "Cargo.toml"
        manifest.write_text(
            '[package]\nname="embedded-snapshot"\nversion="0.1.0"\n'
            'edition="2021"\n[workspace]\n'
        )
        source = self.repo / "src"
        source.mkdir()
        (source / "main.rs").write_text(
            'fn main() { print!("{}{}", '
            'include_str!("../crates/ry-typeshed/vendor/SOURCE"), '
            'include_str!("../crates/ry-typeshed/vendor/base/base.json")); }\n'
        )
        old_base = self.vendor / "base"
        old_base.mkdir()
        (old_base / "base.json").write_text('{"package":"old"}\n')
        real_cargo = shutil.which("cargo")
        self.env.update(REAL_CARGO=real_cargo, FRESHNESS_MANIFEST=str(manifest),
                        CARGO_TARGET_DIR=str(self.repo / "isolated target"))
        result = self.run_sync(0)
        self.assertEqual(result.returncode, 0, result.stderr)
        rebuilt = subprocess.run(
            [real_cargo, "run", "--offline", "--quiet", "--manifest-path", str(manifest)],
            env=self.env, capture_output=True, text=True,
        )
        self.assertEqual(rebuilt.returncode, 0, rebuilt.stderr)
        expected = ((self.vendor / "SOURCE").read_text()
                    + (self.vendor / "base/base.json").read_text())
        self.assertEqual(rebuilt.stdout, expected)
        self.assert_no_staging_directory()

    def test_failed_freshness_refresh_preserves_snapshot(self):
        touch = self.binaries / "touch"
        touch.write_text("#!/usr/bin/env bash\nexit 8\n")
        touch.chmod(0o755)
        result = self.run_sync(0)
        self.assertEqual(result.returncode, 8, result.stderr)
        self.assert_old_snapshot()
        self.assert_no_staging_directory()

    def test_failed_install_restores_snapshot_and_provenance(self):
        self.fail_move("install")
        result = self.run_sync(0)
        self.assertEqual(result.returncode, 7, result.stderr)
        self.assertIn("restored the previous typeshed snapshot", result.stderr)
        self.assert_old_snapshot()
        self.assert_no_staging_directory()

    def test_failed_backup_move_leaves_existing_snapshot(self):
        self.fail_move("backup")
        result = self.run_sync(0)
        self.assertEqual(result.returncode, 7, result.stderr)
        self.assert_old_snapshot()
        self.assert_no_staging_directory()

    def test_term_after_install_move_restores_snapshot(self):
        self.fail_move("interrupt")
        result = self.run_sync(0)
        self.assertEqual(result.returncode, 143, result.stderr)
        self.assert_old_snapshot()
        self.assert_no_staging_directory()

    def test_failed_restore_retains_snapshot_and_reports_recovery_path(self):
        self.fail_move("restore")
        result = self.run_sync(0)
        self.assertEqual(result.returncode, 7, result.stderr)
        backups = list(self.vendor.parent.glob("vendor.backup.*"))
        self.assertEqual(len(backups), 1)
        snapshot = backups[0] / "snapshot"
        self.assert_old_snapshot(snapshot)
        self.assertIn(f"recover it from {snapshot}", result.stderr)
        self.assertFalse(self.vendor.exists())
        self.assertEqual(list(self.vendor.parent.glob("vendor.*")), backups)


if __name__ == "__main__":
    unittest.main()
