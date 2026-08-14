#!/usr/bin/env python3
"""Assert that forbidden internal Cargo dependency edges do not exist.

P38-W3 requirement: ry-config and ry-workspace must not depend on ry-checker.
"""

import subprocess
import sys

FORBIDDEN = {
    "ry-config": ["ry-checker"],
    "ry-workspace": ["ry-checker"],
}


def check_crate(crate: str, forbidden_deps: list[str]) -> bool:
    """Return True if crate has no forbidden dependencies."""
    result = subprocess.run(
        ["cargo", "tree", "-p", crate, "--prefix", "none"],
        capture_output=True,
        text=True,
    )
    deps = result.stdout
    for dep in forbidden_deps:
        if dep in deps:
            print(f"FAIL: {crate} still depends on {dep}")
            return False
    print(f"OK: {crate} has no forbidden dependencies")
    return True


def main() -> int:
    all_ok = True
    for crate, forbidden in FORBIDDEN.items():
        if not check_crate(crate, forbidden):
            all_ok = False

    if all_ok:
        print("\nAll dependency-direction assertions passed.")
        return 0
    else:
        print("\nDependency-direction violations found!")
        return 1


if __name__ == "__main__":
    sys.exit(main())
