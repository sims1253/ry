#!/usr/bin/env python3
"""Reject dependencies from ry-config or ry-workspace on ry-checker."""

import subprocess
import sys


def main() -> int:
    failed = False
    for crate in ("ry-config", "ry-workspace"):
        result = subprocess.run(
            ["cargo", "tree", "-p", crate, "--prefix", "none"],
            stdout=subprocess.PIPE,
            text=True,
            check=True,
        )
        if any(line.split()[:1] == ["ry-checker"] for line in result.stdout.splitlines()):
            print(f"FAIL: {crate} depends on ry-checker")
            failed = True
        else:
            print(f"OK: {crate} has no dependency on ry-checker")
    return int(failed)


if __name__ == "__main__":
    sys.exit(main())
