#!/usr/bin/env python3
"""Ratchet GNU R's parse() accept/reject verdict against ry's RY000.

For every `.R`/`.r` file under each named source tree, GNU R's parser
(`ecosystem/parse-accept.R`, one R process) and ry (`check`, explicit file
paths, so test fixtures and .Rbuildignore'd files count too) each accept or
reject the file. ry rejects a file when it reports RY000 there. The ledger
records per-source file and rejection counts and every file where the two
disagree; `check` fails on any change to them for the processed sources, so
a new false RY000 on valid R and newly accepted R-rejected input both fail.
R's messages and reviewer notes are kept for reading only.

Usage:
  parse_differential.py {check,update} --binary RY --ledger FILE
      --corpus NAME NAME=DIR [NAME=DIR ...]
"""

from __future__ import annotations

import argparse
import copy
import difflib
import json
import os
import subprocess
import sys
import tempfile
from pathlib import Path

SCRIPT = Path(__file__).with_name("parse-accept.R")
# Explicit paths per ry invocation, well under the platform argument limit.
CHUNK = 1000


def rendered(document: dict) -> str:
    return json.dumps(document, indent=2, ensure_ascii=True) + "\n"


def source_files(root: Path) -> list[str]:
    files = []
    for directory, subdirectories, names in os.walk(root):
        subdirectories[:] = [name for name in subdirectories if name != ".git"]
        for name in names:
            path = Path(directory, name)
            if path.suffix in (".R", ".r") and path.is_file() and not path.is_symlink():
                files.append(path.relative_to(root).as_posix())
    return sorted(files)


def r_verdicts(paths: list[Path]) -> dict[Path, str | None]:
    """R's first error line for each rejected path, None for accepted ones."""
    with tempfile.TemporaryDirectory() as temporary:
        listing = Path(temporary, "files.txt")
        output = Path(temporary, "verdicts.tsv")
        listing.write_text("".join(f"{path}\n" for path in paths), encoding="utf-8")
        subprocess.run(
            ["Rscript", "--vanilla", str(SCRIPT), str(listing), str(output)],
            check=True,
        )
        verdicts: dict[Path, str | None] = {}
        for line in output.read_text(encoding="utf-8", errors="replace").splitlines():
            path, verdict, *message = line.split("\t")
            verdicts[Path(path)] = (message[0] if message else "") if verdict == "REJECT" else None
    if set(verdicts) != set(paths):
        raise SystemExit("parse differential: R did not classify every file")
    return verdicts


def ry_rejections(binary: str, root: Path, files: list[str]) -> set[str]:
    rejected = set()
    for start in range(0, len(files), CHUNK):
        result = subprocess.run(
            [binary, "check", "--output-format", "json", "--exit-zero", "--",
             *files[start:start + CHUNK]],
            cwd=root,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
        # ry notes each parse error on stderr; show it only on failure.
        if result.returncode != 0:
            sys.stderr.write(result.stderr)
            raise SystemExit(f"parse differential: ry failed in {root}")
        for diagnostic in json.loads(result.stdout):
            if diagnostic["code"] == "RY000":
                rejected.add(Path(diagnostic["path"]).as_posix().removeprefix("./"))
    return rejected


def observe(binary: str, sources: dict[str, Path]) -> tuple[dict, list]:
    listed = {name: source_files(root) for name, root in sources.items()}
    verdicts = r_verdicts(
        [root / path for name, root in sources.items() for path in listed[name]]
    )
    packages, disagreements = {}, []
    for name, root in sources.items():
        files = listed[name]
        ry_rejected = ry_rejections(binary, root, files) if files else set()
        r_rejected = {path for path in files if verdicts[root / path] is not None}
        packages[name] = {
            "files": len(files),
            "r_rejected": len(r_rejected),
            "ry_rejected": len(ry_rejected & set(files)),
        }
        for path in sorted(r_rejected ^ ry_rejected):
            entry = {
                "package": name,
                "path": path,
                "r": "reject" if path in r_rejected else "accept",
                "ry": "reject" if path in ry_rejected else "accept",
            }
            if path in r_rejected:
                entry["r_message"] = verdicts[root / path]
            disagreements.append(entry)
    return packages, disagreements


def r_version() -> str:
    result = subprocess.run(
        ["Rscript", "--vanilla", "-e", "cat(as.character(getRversion()))"],
        check=True, stdout=subprocess.PIPE, text=True,
    )
    return result.stdout.strip()


def with_totals(corpus: dict) -> dict:
    packages = corpus["packages"]
    return {
        "files": sum(entry["files"] for entry in packages.values()),
        "r_rejected": sum(entry["r_rejected"] for entry in packages.values()),
        "ry_rejected": sum(entry["ry_rejected"] for entry in packages.values()),
        "disagreements": len(corpus["disagreement_files"]),
        "packages": dict(sorted(packages.items())),
        "disagreement_files": corpus["disagreement_files"],
    }


def identity(entry: dict) -> tuple:
    return entry["package"], entry["path"], entry["r"], entry["ry"]


def gated(ledger: dict) -> dict:
    """The ledger without the fields kept only for reading."""
    ledger = copy.deepcopy(ledger)
    ledger.pop("r_version", None)
    for corpus in ledger["corpora"].values():
        for entry in corpus["disagreement_files"]:
            entry.pop("r_message", None)
            entry.pop("note", None)
    return ledger


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("mode", choices=("check", "update"))
    parser.add_argument("--binary", required=True)
    parser.add_argument("--ledger", type=Path, required=True)
    parser.add_argument("--corpus", required=True)
    parser.add_argument("sources", nargs="+", metavar="NAME=DIR")
    args = parser.parse_args()

    sources = {}
    for source in args.sources:
        name, separator, directory = source.partition("=")
        if not separator or not name or name in sources:
            raise SystemExit(f"parse differential: bad or duplicate source {source!r}")
        sources[name] = Path(directory).resolve()

    if args.ledger.exists():
        committed = json.loads(args.ledger.read_text(encoding="utf-8"))
    else:
        committed = {"schema_version": 1, "r_version": "", "corpora": {}}
    packages, disagreements = observe(args.binary, sources)

    previous = committed["corpora"].get(
        args.corpus, {"packages": {}, "disagreement_files": []}
    )
    notes = {
        identity(entry): entry["note"]
        for entry in previous["disagreement_files"]
        if "note" in entry
    }
    for entry in disagreements:
        if identity(entry) in notes:
            entry["note"] = notes[identity(entry)]
    # Sources outside this run (another tier) keep their committed entries.
    merged_packages = {
        name: entry for name, entry in previous["packages"].items() if name not in sources
    }
    merged_packages.update(packages)
    merged_disagreements = [
        entry for entry in previous["disagreement_files"] if entry["package"] not in sources
    ] + disagreements
    merged_disagreements.sort(key=identity)

    candidate = copy.deepcopy(committed)
    candidate["r_version"] = r_version()
    candidate["corpora"][args.corpus] = with_totals(
        {"packages": merged_packages, "disagreement_files": merged_disagreements}
    )
    candidate["corpora"] = dict(sorted(candidate["corpora"].items()))

    processed = sum(entry["files"] for entry in packages.values())
    if args.mode == "update":
        args.ledger.write_text(rendered(candidate), encoding="utf-8")
        print(
            f"parse differential: updated {args.corpus} ({processed} files, "
            f"{len(disagreements)} disagreements) in {args.ledger}"
        )
        return 0

    if gated(candidate) == gated(committed):
        print(
            f"parse differential: {processed} files match ({len(disagreements)} "
            "recorded disagreements)"
        )
        return 0
    sys.stderr.writelines(
        difflib.unified_diff(
            rendered(committed).splitlines(keepends=True),
            rendered(candidate).splitlines(keepends=True),
            fromfile=str(args.ledger),
            tofile=f"{args.ledger} (observed)",
            n=3,
        )
    )
    print(
        "parse differential: GNU R and ry accept/reject verdicts drifted; "
        "a new disagreement is a parser bug to fix, a resolved one needs the "
        "ledger regenerated with ecosystem/run.sh",
        file=sys.stderr,
    )
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
