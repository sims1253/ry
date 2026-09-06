#!/usr/bin/env python3
"""Hash release executables from verified cargo-dist archives without extracting."""

import argparse
import hashlib
from pathlib import Path
import re
import stat
import tarfile
import tomllib
import zipfile


def binary_digest(archive: Path, member: str) -> str:
    checksum = archive.with_name(archive.name + ".sha256").read_text()
    match = re.fullmatch(r"([0-9a-fA-F]{64}) [ *]" + re.escape(archive.name) + r"\n*", checksum)
    if match is None:
        raise ValueError(f"Malformed archive checksum: {archive.name}")
    with archive.open("rb") as source:
        actual = hashlib.file_digest(source, "sha256").hexdigest()
    if actual != match[1].lower():
        raise ValueError(f"Archive checksum mismatch: {archive.name}")

    if archive.name.endswith(".zip"):
        with zipfile.ZipFile(archive) as source:
            matches = [entry for entry in source.infolist() if entry.filename == member]
            if len(matches) != 1 or matches[0].is_dir() or stat.S_ISLNK(matches[0].external_attr >> 16):
                raise ValueError(f"Expected one regular executable {member} in {archive.name}")
            with source.open(matches[0]) as binary:
                return hashlib.file_digest(binary, "sha256").hexdigest()
    with tarfile.open(archive, "r:gz") as source:
        matches = [entry for entry in source.getmembers() if entry.name == member]
        if len(matches) != 1 or not matches[0].isfile():
            raise ValueError(f"Expected one regular executable {member} in {archive.name}")
        with source.extractfile(matches[0]) as binary:
            return hashlib.file_digest(binary, "sha256").hexdigest()


def generate(artifacts: Path, output: Path, targets: list[str]) -> list[Path]:
    # Validate every target first. A missing or invalid archive prevents a
    # partial set of sidecars from reaching the release upload.
    sidecars = {}
    for target in targets:
        if re.fullmatch(r"[a-zA-Z0-9_-]+", target) is None:
            raise ValueError(f"Invalid target: {target}")
        stem = f"ry-cli-{target}"
        windows = target.endswith("-pc-windows-msvc")
        binary = "ry.exe" if windows else "ry"
        member = binary if windows else f"{stem}/{binary}"
        archive = artifacts / (stem + (".zip" if windows else ".tar.gz"))
        digest = binary_digest(archive, member)
        sidecars[f"{stem}.bin.sha256"] = f"{digest}  {binary}\n"
    if not sidecars:
        raise ValueError("No release targets configured")
    output.mkdir(parents=True, exist_ok=True)
    for name, contents in sidecars.items():
        (output / name).write_text(contents, encoding="ascii")
    return [output / name for name in sidecars]


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--artifacts", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--config", type=Path, default=Path("dist-workspace.toml"))
    args = parser.parse_args()
    with args.config.open("rb") as source:
        targets = tomllib.load(source)["dist"]["targets"]
    for sidecar in generate(args.artifacts, args.output, targets):
        print(sidecar)


if __name__ == "__main__":
    main()
