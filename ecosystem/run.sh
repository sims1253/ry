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

for command in cargo git Rscript; do
  command -v "$command" >/dev/null 2>&1 || {
    echo "ecosystem: required command not found: $command" >&2
    exit 2
  }
done

# Snapshots must not depend on which R packages are installed on the
# machine that generates them: disable ry's installed-library resolution.
export RY_NO_INSTALLED_LIBRARIES=1

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
  Rscript - "$json" "$package_dir" "$generated_dir/$name.txt" "$generated_dir/$name.full.txt" <<'RS'
args <- commandArgs(trailingOnly = TRUE)
json_path <- args[[1]]
package_dir <- normalizePath(args[[2]], winslash = "/", mustWork = TRUE)
stable_path <- args[[3]]
full_path <- args[[4]]

diagnostics <- jsonlite::fromJSON(json_path, simplifyDataFrame = FALSE)
stable <- character(0)
full <- character(0)
prefix_dir <- paste0(package_dir, "/")
for (diagnostic in diagnostics) {
  path <- diagnostic$path
  absolute <- tryCatch(
    normalizePath(path, winslash = "/", mustWork = FALSE),
    error = function(e) path
  )
  relative <- if (startsWith(absolute, prefix_dir)) {
    substring(absolute, nchar(prefix_dir) + 1L)
  } else {
    path
  }
  prefix <- sprintf("%s:%s:%s %s", relative, diagnostic$line, diagnostic$column, diagnostic$code)
  stable <- c(stable, prefix)
  message <- paste(strsplit(trimws(as.character(diagnostic$message)), "\\s+")[[1]], collapse = " ")
  full <- c(full, paste(prefix, message))
}

write_sorted <- function(lines, path) {
  handle <- file(path, open = "wb")
  on.exit(close(handle))
  if (length(lines)) {
    writeLines(sort(lines, method = "radix"), handle, sep = "\n", useBytes = TRUE)
  }
}
write_sorted(stable, stable_path)
write_sorted(full, full_path)
RS
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

Rscript - "$packages_file" "$summary_input" "$generated_dir/SUMMARY.md" <<'RS'
args <- commandArgs(trailingOnly = TRUE)
packages_file <- args[[1]]
reports_dir <- args[[2]]
output_path <- args[[3]]

raw <- readLines(packages_file, encoding = "UTF-8", warn = FALSE)
raw <- raw[nzchar(raw) & !startsWith(raw, "#")]
package_order <- vapply(strsplit(raw, "\t", fixed = TRUE), `[[`, character(1), 1L)

counts <- list()
for (package in package_order) {
  report <- file.path(reports_dir, paste0(package, ".txt"))
  if (!file.exists(report)) next
  entries <- readLines(report, encoding = "UTF-8", warn = FALSE)
  entries <- entries[nzchar(entries)]
  codes <- vapply(strsplit(entries, "[ \t]+"), `[[`, character(1), 2L)
  counts[[package]] <- table(codes)
}

all_codes <- sort(unique(unlist(lapply(counts, names), use.names = FALSE)), method = "radix")
packages <- names(counts)
lines <- c(
  "# Ecosystem diagnostic summary",
  "",
  "Counts are generated from the committed message-free reports.",
  "",
  paste0("| Rule | ", paste(packages, collapse = " | "), " | Total |"),
  paste0("| :--- | ", paste(rep("---:", length(packages)), collapse = " | "), " | ---: |")
)
for (code in all_codes) {
  values <- vapply(packages, function(package) {
    count <- counts[[package]][code]
    if (is.na(count)) 0L else as.integer(count)
  }, integer(1))
  lines <- c(lines, paste0(
    "| ", code, " | ", paste(values, collapse = " | "), " | ", sum(values), " |"
  ))
}
if (!length(all_codes)) {
  lines <- c(lines, "", "No diagnostics were emitted by the available package snapshots.")
}
handle <- file(output_path, open = "wb")
writeLines(lines, handle, sep = "\n", useBytes = TRUE)
close(handle)
RS

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
