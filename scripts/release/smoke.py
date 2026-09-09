#!/usr/bin/env python3
"""Verify and run one native release archive before publication."""

import argparse
import hashlib
import json
from pathlib import Path
import platform
import shutil
import subprocess
import tarfile
import tempfile
import tomllib
import zipfile

from binary_checksums import binary_digest


def smoke(artifacts: Path, target: str, version: str) -> dict:
    machine = platform.machine().lower()
    expected_machines = {"aarch64", "arm64"} if target.startswith("aarch64-") else {"x86_64", "amd64"}
    if machine not in expected_machines:
        raise ValueError(f"Target {target} needs a native runner; got {machine}")
    stem = f"ry-cli-{target}"
    windows = target.endswith("-pc-windows-msvc")
    name = "ry.exe" if windows else "ry"
    member = name if windows else f"{stem}/{name}"
    archive = artifacts / (stem + (".zip" if windows else ".tar.gz"))
    digest = binary_digest(archive, member)
    expected = (artifacts / f"{stem}.bin.sha256").read_text().strip()
    if expected != f"{digest}  {name}":
        raise ValueError(f"Executable checksum mismatch: {target}")

    with tempfile.TemporaryDirectory(prefix="ry-release-smoke-") as directory:
        root = Path(directory)
        binary = root / name
        # Copy only the verified regular executable. Archive paths never
        # determine extraction destinations.
        if windows:
            with zipfile.ZipFile(archive) as source, source.open(member) as stream:
                with binary.open("wb") as output:
                    shutil.copyfileobj(stream, output)
        else:
            with tarfile.open(archive, "r:gz") as source, source.extractfile(member) as stream:
                with binary.open("wb") as output:
                    shutil.copyfileobj(stream, output)
            binary.chmod(0o755)
        if hashlib.sha256(binary.read_bytes()).hexdigest() != digest:
            raise ValueError("Extracted executable differs from verified archive")
        actual = subprocess.run(
            [str(binary), "version"], check=True, capture_output=True, text=True, timeout=30
        ).stdout.strip()
        if actual != f"ry {version}":
            raise ValueError(f"Expected ry {version}, got {actual!r}")
        source = root / "smoke.R"
        source.write_text('x <- "hello"\nx + 1L\n', encoding="utf-8")
        command = [str(binary), "check", str(source), "--output-format", "json"]
        bad = subprocess.run(command, cwd=root, capture_output=True, text=True, timeout=30)
        diagnostics = json.loads(bad.stdout)
        if bad.returncode != 1 or len(diagnostics) != 1:
            raise ValueError(f"Expected one failing diagnostic: {bad.stdout} {bad.stderr}")
        diagnostic = diagnostics[0]
        if (diagnostic["code"], diagnostic["line"], diagnostic["column"]) != ("RY040", 2, 1):
            raise ValueError(f"Unexpected diagnostic: {diagnostic}")
        source.write_text("x <- 1L\nx + 1L\n", encoding="utf-8")
        good = subprocess.run(command, cwd=root, capture_output=True, text=True, timeout=30)
        if good.returncode != 0 or json.loads(good.stdout) != []:
            raise ValueError(f"Corrected file is not clean: {good.stdout} {good.stderr}")
        installed_bytes = binary.stat().st_size
    return {
        "target": target,
        "host": platform.platform(),
        "machine": machine,
        "version": actual,
        "archive_bytes": archive.stat().st_size,
        "installed_bytes": installed_bytes,
        "binary_sha256": digest,
        "invalid_exit": bad.returncode,
        "corrected_exit": good.returncode,
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--artifacts", type=Path, required=True)
    parser.add_argument("--target", required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    with Path("dist-workspace.toml").open("rb") as source:
        targets = tomllib.load(source)["dist"]["targets"]
    if args.target not in targets:
        raise ValueError(f"Unconfigured release target: {args.target}")
    with Path("Cargo.toml").open("rb") as source:
        version = tomllib.load(source)["workspace"]["package"]["version"]
    result = smoke(args.artifacts, args.target, version)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
