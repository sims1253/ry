import hashlib
import io
from pathlib import Path
import tarfile
import tempfile
import tomllib
import unittest
import zipfile

from binary_checksums import generate


class BinaryChecksumsTest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.output = self.root / "output"

    def archive(self, target, content=b"server bytes", member=None, symlink=False, duplicate=False):
        stem = f"ry-cli-{target}"
        windows = target.endswith("-pc-windows-msvc")
        member = member or ("ry.exe" if windows else f"{stem}/ry")
        archive = self.root / (stem + (".zip" if windows else ".tar.gz"))
        if windows:
            with zipfile.ZipFile(archive, "w") as output:
                entry = zipfile.ZipInfo(member)
                if symlink:
                    entry.external_attr = 0o120777 << 16
                output.writestr(entry, content)
                if duplicate:
                    output.writestr(entry, content)
        else:
            with tarfile.open(archive, "w:gz") as output:
                entry = tarfile.TarInfo(member)
                entry.size = len(content)
                if symlink:
                    entry.type = tarfile.SYMTYPE
                    entry.linkname = "elsewhere"
                output.addfile(entry, io.BytesIO(content))
                if duplicate:
                    output.addfile(entry, io.BytesIO(content))
        with archive.open("rb") as source:
            digest = hashlib.file_digest(source, "sha256").hexdigest()
        archive.with_name(archive.name + ".sha256").write_text(f"{digest} *{archive.name}\n\n")
        return archive

    def test_all_configured_targets_hash_executable_not_archive(self):
        config = Path(__file__).resolve().parents[2] / "dist-workspace.toml"
        with config.open("rb") as source:
            targets = tomllib.load(source)["dist"]["targets"]
        for target in targets:
            self.archive(target)
        paths = generate(self.root, self.output, targets)
        self.assertEqual(len(paths), len(targets))
        for target, path in zip(targets, paths):
            filename = "ry.exe" if "windows" in target else "ry"
            self.assertEqual(path.name, f"ry-cli-{target}.bin.sha256")
            self.assertEqual(path.read_text(), f"{hashlib.sha256(b'server bytes').hexdigest()}  {filename}\n")

    def test_missing_or_changed_archive_prevents_all_output(self):
        first = "x86_64-unknown-linux-gnu"
        second = "aarch64-apple-darwin"
        self.archive(first)
        with self.assertRaises(FileNotFoundError):
            generate(self.root, self.output, [first, second])
        self.assertFalse(self.output.exists())
        archive = self.archive(second)
        with archive.open("ab") as output:
            output.write(b"modified")
        with self.assertRaisesRegex(ValueError, "Archive checksum mismatch"):
            generate(self.root, self.output, [first, second])
        self.assertFalse(self.output.exists())

    def test_wrong_member_links_and_duplicates_are_refused(self):
        for target in ["x86_64-unknown-linux-gnu", "x86_64-pc-windows-msvc"]:
            for kwargs in [{"member": "wrong/ry"}, {"symlink": True}, {"duplicate": True}]:
                with self.subTest(target=target, kwargs=kwargs):
                    self.archive(target, **kwargs)
                    with self.assertRaisesRegex(ValueError, "Expected one regular executable"):
                        generate(self.root, self.output, [target])
                    self.assertFalse(self.output.exists())

    def test_bad_archive_sidecars_are_refused(self):
        target = "x86_64-unknown-linux-gnu"
        archive = self.archive(target)
        for contents in ["invalid", "0" * 64 + " *wrong.tar.gz\n", "0" * 64 + f" *{archive.name}\nextra\n"]:
            archive.with_name(archive.name + ".sha256").write_text(contents)
            with self.assertRaisesRegex(ValueError, "Malformed archive checksum"):
                generate(self.root, self.output, [target])


if __name__ == "__main__":
    unittest.main()
