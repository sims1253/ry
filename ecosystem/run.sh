#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ecosystem_dir="$root/ecosystem"
cache_dir="$ecosystem_dir/.cache"
reports_dir="$ecosystem_dir/reports"
packages_file="$ecosystem_dir/packages.txt"
check=false
local_only=false

usage() {
  cat <<'EOF'
Usage: ecosystem/run.sh [--check] [--local]

  --check  Compare generated reports with committed snapshots.
  --local  Check only locally vendored sources (currently glue); do not clone.
EOF
}

while (($#)); do
  case "$1" in
    --check) check=true ;;
    --local) local_only=true ;;
    -h|--help) usage; exit 0 ;;
    *) echo "unknown argument: $1" >&2; usage >&2; exit 2 ;;
  esac
  shift
done

for command in cargo git python3; do
  command -v "$command" >/dev/null 2>&1 || {
    echo "ecosystem: required command not found: $command" >&2
    exit 2
  }
done

mkdir -p "$cache_dir" "$reports_dir"
work_dir="$(mktemp -d "${TMPDIR:-/tmp}/ry-ecosystem.XXXXXX")"
trap 'rm -rf "$work_dir"' EXIT
generated_dir="$work_dir/reports"
mkdir -p "$generated_dir"

binary="${RY_BINARY:-$root/target/release/ry}"
if [[ ! -x "$binary" ]]; then
  cargo build --release --locked -p ry-cli --bin ry --manifest-path "$root/Cargo.toml"
fi

processed_packages=()
while IFS=$'\t' read -r name url pinned_ref; do
  [[ -z "${name:-}" || "$name" == \#* ]] && continue

  if $local_only; then
    if [[ "$name" != "glue" ]]; then
      continue
    fi
    package_dir="$root/crates/ry-checker/testdata/vendor/glue"
  else
    package_dir="$cache_dir/$name"
    if [[ ! -d "$package_dir/.git" ]]; then
      echo "ecosystem: cloning $name at $pinned_ref"
      git clone --depth 1 --branch "$pinned_ref" "$url" "$package_dir"
    else
      echo "ecosystem: refreshing $name at $pinned_ref"
      git -C "$package_dir" fetch --depth 1 origin "$pinned_ref"
      git -C "$package_dir" checkout --detach FETCH_HEAD
    fi
  fi

  if [[ ! -d "$package_dir/R" ]]; then
    echo "ecosystem: $name has no R/ directory at $pinned_ref" >&2
    exit 1
  fi

  echo "ecosystem: checking $name"
  json="$work_dir/$name.json"
  "$binary" check --output-format json --exit-zero "$package_dir/R" > "$json"
  python3 - "$json" "$package_dir" "$generated_dir/$name.txt" "$generated_dir/$name.full.txt" <<'PY'
import json
import os
import sys

json_path, package_dir, stable_path, full_path = sys.argv[1:]
with open(json_path, encoding="utf-8") as source:
    diagnostics = json.load(source)

stable = []
full = []
package_dir = os.path.abspath(package_dir)
for diagnostic in diagnostics:
    path = diagnostic["path"]
    absolute_path = path if os.path.isabs(path) else os.path.abspath(path)
    try:
        relative_path = os.path.relpath(absolute_path, package_dir)
    except ValueError:
        relative_path = path
    relative_path = relative_path.replace(os.sep, "/")
    prefix = f'{relative_path}:{diagnostic["line"]}:{diagnostic["column"]} {diagnostic["code"]}'
    stable.append(prefix)
    message = " ".join(str(diagnostic["message"]).split())
    full.append(f"{prefix} {message}")

for output_path, lines in ((stable_path, stable), (full_path, full)):
    with open(output_path, "w", encoding="utf-8", newline="\n") as output:
        for line in sorted(lines):
            output.write(line + "\n")
PY
  processed_packages+=("$name")
done < "$packages_file"

if ((${#processed_packages[@]} == 0)); then
  echo "ecosystem: no packages were processed" >&2
  exit 1
fi

summary_input="$work_dir/summary-input"
mkdir -p "$summary_input"
if compgen -G "$reports_dir/*.txt" >/dev/null; then
  cp "$reports_dir"/*.txt "$summary_input/"
fi
for name in "${processed_packages[@]}"; do
  cp "$generated_dir/$name.txt" "$summary_input/$name.txt"
done
rm -f "$summary_input/SUMMARY.md"

python3 - "$packages_file" "$summary_input" "$generated_dir/SUMMARY.md" <<'PY'
from collections import Counter
from pathlib import Path
import sys

packages_file, reports_dir, output_path = map(Path, sys.argv[1:])
package_order = []
for line in packages_file.read_text(encoding="utf-8").splitlines():
    if not line or line.startswith("#"):
        continue
    package_order.append(line.split("\t", 1)[0])

counts = {}
for package in package_order:
    report = reports_dir / f"{package}.txt"
    if not report.exists():
        continue
    counter = Counter()
    for line in report.read_text(encoding="utf-8").splitlines():
        if line:
            counter[line.split()[1]] += 1
    counts[package] = counter

codes = sorted({code for counter in counts.values() for code in counter})
packages = list(counts)
lines = [
    "# Ecosystem diagnostic summary",
    "",
    "Counts are generated from the committed message-free reports.",
    "",
    "| Rule | " + " | ".join(packages) + " | Total |",
    "| :--- | " + " | ".join("---:" for _ in packages) + " | ---: |",
]
for code in codes:
    values = [counts[package][code] for package in packages]
    lines.append(f'| {code} | ' + " | ".join(map(str, values)) + f" | {sum(values)} |")
if not codes:
    lines.extend(["", "No diagnostics were emitted by the available package snapshots."])
output_path.write_text("\n".join(lines) + "\n", encoding="utf-8")
PY

if ! $check; then
  for name in "${processed_packages[@]}"; do
    cp "$generated_dir/$name.txt" "$reports_dir/$name.txt"
    cp "$generated_dir/$name.full.txt" "$reports_dir/$name.full.txt"
  done
  cp "$generated_dir/SUMMARY.md" "$reports_dir/SUMMARY.md"
  echo "ecosystem: updated reports for ${processed_packages[*]}"
  exit 0
fi

drift=0
for name in "${processed_packages[@]}"; do
  expected="$reports_dir/$name.txt"
  actual="$generated_dir/$name.txt"
  if [[ ! -f "$expected" ]] || ! cmp -s "$expected" "$actual"; then
    echo "ecosystem: report drift for $name" >&2
    if [[ -f "$expected" ]]; then
      diff -u "$expected" "$actual" || true
    else
      diff -u /dev/null "$actual" || true
    fi
    drift=1
  fi
done
if [[ ! -f "$reports_dir/SUMMARY.md" ]] || ! cmp -s "$reports_dir/SUMMARY.md" "$generated_dir/SUMMARY.md"; then
  echo "ecosystem: report drift for SUMMARY.md" >&2
  if [[ -f "$reports_dir/SUMMARY.md" ]]; then
    diff -u "$reports_dir/SUMMARY.md" "$generated_dir/SUMMARY.md" || true
  else
    diff -u /dev/null "$generated_dir/SUMMARY.md" || true
  fi
  drift=1
fi

if ((drift)); then
  echo "ecosystem: regenerate reports with ecosystem/run.sh and commit them" >&2
  exit 1
fi
echo "ecosystem: committed reports are current"
