#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ecosystem_dir="$root/ecosystem"
cache_dir="$ecosystem_dir/.cache"
reports_dir="$ecosystem_dir/reports"
packages_file="$ecosystem_dir/packages.txt"
audit_corpus=""
check=false
local_only=false
tier=full

usage() {
  cat <<'EOF'
Usage: ecosystem/run.sh [--check] [--local] [--manifest FILE]
                        [--ledger FILE] [--tier {fast,full}]

  --check        Compare generated reports with committed snapshots.
  --local        Check only the locally vendored glue R/ sources; do not clone.
  --manifest F   Package manifest to use (default: ecosystem/packages.txt).
  --ledger F     Corpus ledger to reconcile against. Defaults to the manifest's
                 `# ledger:` directive, then docs/corpus/tidyverse-0.7.1.json.
  --tier T       fast = only the manifest's fast-tier (signal-dense) packages;
                 full = every package in the manifest (default).

A manifest may carry a `# ledger: <path-relative-to-repo>` directive selecting
the corpus its hermetic root reports reconcile against, and a `# === full tier`
marker separating the fast-tier packages from the rest. Multiple ledgers
(tidyverse-0.7.1, posit-0.8.0) coexist via their own manifests.
EOF
}

while (($#)); do
  case "$1" in
    --check) check=true ;;
    --local) local_only=true ;;
    --manifest) [[ $# -ge 2 ]] || { echo "ecosystem: --manifest requires a value" >&2; usage >&2; exit 2; }; packages_file="$2"; shift ;;
    --ledger) [[ $# -ge 2 ]] || { echo "ecosystem: --ledger requires a value" >&2; usage >&2; exit 2; }; audit_corpus="$2"; shift ;;
    --tier) [[ $# -ge 2 ]] || { echo "ecosystem: --tier requires a value" >&2; usage >&2; exit 2; }; tier="$2"; shift ;;
    -h|--help) usage; exit 0 ;;
    *) echo "unknown argument: $1" >&2; usage >&2; exit 2 ;;
  esac
  shift
done

if [[ "$tier" != "fast" && "$tier" != "full" ]]; then
  echo "ecosystem: --tier must be 'fast' or 'full' (got: $tier)" >&2
  exit 2
fi
if [[ ! -r "$packages_file" ]]; then
  echo "ecosystem: manifest not found or not readable: $packages_file" >&2
  exit 2
fi

# Resolve the ledger: an explicit --ledger wins; otherwise a manifest may
# declare its ledger with `# ledger: <relative-path>`; else default to the
# tidyverse ledger for backward compatibility.
if [[ -z "$audit_corpus" ]]; then
  directive="$(grep -m1 '^# ledger:[[:space:]]' "$packages_file" 2>/dev/null \
    | sed 's/^# ledger:[[:space:]]*//; s/[[:space:]]*$//' || true)"
  if [[ -n "$directive" ]]; then
    audit_corpus="$root/$directive"
  else
    audit_corpus="$root/docs/corpus/tidyverse-0.7.1.json"
  fi
fi

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

write_report() {
  local json="$1"
  local package_dir="$2"
  local stable_path="$3"
  local full_path="$4"

  Rscript - "$json" "$package_dir" "$stable_path" "$full_path" <<'RS'
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
    sub("^\\./", "", path)
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
}

run_suite() {
  local name="$1"
  local package_dir="$2"
  local target="$3"
  local report_stem="$4"
  local suite_label="$5"
  local json="$work_dir/$report_stem.json"

  echo "ecosystem: checking $name ($suite_label)"
  "$binary" check --output-format json --exit-zero "$target" > "$json"
  write_report \
    "$json" \
    "$package_dir" \
    "$generated_dir/$report_stem.txt" \
    "$generated_dir/$report_stem.full.txt"
  processed_reports+=("$report_stem")
}

processed_packages=()
processed_reports=()
root_packages=()
all_names=()
# Pre-validate the entire manifest for slug collisions, including entries
# past the full-tier marker that --tier fast would skip.
while IFS= read -r line; do
  case "$line" in *"=== full tier"*) continue ;; esac
  IFS=$'\t' read -r slug _ <<< "$line"
  [[ -z "${slug:-}" || "$slug" == \#* ]] && continue
  for seen in "${all_names[@]}"; do
    if [[ "$seen" == "$slug" ]]; then
      echo "ecosystem: duplicate package slug '$slug' in $packages_file; manifests must be collision-safe" >&2
      exit 1
    fi
  done
  all_names+=("$slug")
done < "$packages_file"

seen_names=()
# Read the raw line first so we can honour the manifest's tier marker (a
# `# === full tier` comment) and detect collisions before cloning anything.
while IFS= read -r raw || [[ -n "${raw:-}" ]]; do
  case "$raw" in
    *"=== full tier"*)
      # Everything after this marker is the full tier only.
      if [[ "$tier" == "fast" ]]; then
        break
      fi
      continue
      ;;
  esac
  # Inline trailing comments (tier/star notes) live in a 4th tab column.
  IFS=$'\t' read -r name url pinned_ref _ <<< "$raw"
  [[ -z "${name:-}" || "$name" == \#* ]] && continue
  if [[ -z "${url:-}" || -z "${pinned_ref:-}" ]]; then
    echo "ecosystem: manifest entry for '$name' needs a URL and a pinned ref" >&2
    exit 1
  fi
  # The corpus is a regression benchmark: a ledger reconciles diagnostic
  # identities against the exact source that produced them. A branch name
  # or tag can move upstream, which would silently re-point the benchmark
  # and make a ledger delta unattributable. Only a full commit ID pins.
  if ! [[ "$pinned_ref" =~ ^[0-9a-f]{40}$ ]]; then
    echo "ecosystem: manifest entry for '$name' pins '$pinned_ref'; a corpus entry needs a full 40-character commit ID, not a branch or tag" >&2
    exit 1
  fi
  # Collision check is now in the pre-validation loop above.
  seen_names+=("$name")

  if $local_only; then
    if [[ "$name" != "glue" ]]; then
      continue
    fi
    package_dir="$root/crates/ry-checker/testdata/vendor/glue"
  else
    package_dir="$cache_dir/$name"
    if [[ ! -d "$package_dir/.git" ]]; then
      echo "ecosystem: cloning $name"
      rm -rf "$package_dir"
      if ! git clone --filter=blob:none --no-checkout "$url" "$package_dir"; then
        rm -rf "$package_dir"
        exit 1
      fi
    fi
    echo "ecosystem: refreshing $name at $pinned_ref"
    git -C "$package_dir" fetch --depth 1 origin "$pinned_ref"
    git -C "$package_dir" checkout --detach --force FETCH_HEAD
    git -C "$package_dir" clean -fdx
  fi

  if [[ ! -d "$package_dir/R" ]]; then
    echo "ecosystem: $name has no R/ directory at $pinned_ref" >&2
    exit 1
  fi

  run_suite "$name" "$package_dir" "$package_dir/R" "$name" "R/"
  if ! $local_only; then
    run_suite "$name" "$package_dir" "$package_dir" "$name.root" "package root"
    root_packages+=("$name")
  fi
  processed_packages+=("$name")
done < "$packages_file"

if ((${#processed_packages[@]} == 0)); then
  echo "ecosystem: no packages were processed" >&2
  exit 1
fi

generate_summary() {
  local suffix="$1"
  local output_name="$2"
  local scope_description="$3"
  local summary_input="$work_dir/summary-input-${output_name//[^A-Za-z0-9]/-}"
  mkdir -p "$summary_input"
  if compgen -G "$reports_dir/*.txt" >/dev/null; then
    cp "$reports_dir"/*.txt "$summary_input/"
  fi
  for report_stem in "${processed_reports[@]}"; do
    cp "$generated_dir/$report_stem.txt" "$summary_input/$report_stem.txt"
  done

  Rscript - "$packages_file" "$summary_input" "$generated_dir/$output_name" "$suffix" "$scope_description" <<'RS'
args <- commandArgs(trailingOnly = TRUE)
packages_file <- args[[1]]
reports_dir <- args[[2]]
output_path <- args[[3]]
suffix <- args[[4]]
scope_description <- args[[5]]

raw <- readLines(packages_file, encoding = "UTF-8", warn = FALSE)
raw <- raw[nzchar(raw) & !startsWith(raw, "#")]
package_order <- vapply(strsplit(raw, "\t", fixed = TRUE), `[[`, character(1), 1L)

counts <- list()
for (package in package_order) {
  report <- file.path(reports_dir, paste0(package, suffix, ".txt"))
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
  scope_description,
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
}

generate_summary "" "SUMMARY.md" \
  "Production-source suite: each package's \`R/\` directory only."
if ! $local_only; then
  generate_summary ".root" "SUMMARY.root.md" \
    "Package-root suite: production, tests, and other checked R sources."
fi

# Each ledger pins every diagnostic identity in the audited packages' hermetic
# root reports rather than an aggregate count, so removing one finding cannot
# be mistaken for removing another. The tidyverse ledger is a strict hermetic
# baseline (`reconciliation: hermetic`, the default when the field is absent):
# any missing/unowned identity fails the build. The posit ledger is an audit
# transcript of an installed-library run (`reconciliation: audit-transcript`):
# the hermetic-vs-audit delta is reported for visibility but does not gate the
# build, since RY_NO_INSTALLED_LIBRARIES=1 legitimately differs from the
# installed-library audit. In both modes findings labelled `true_positive` are
# checked explicitly so a real bug disappearing is always surfaced.
if ! $local_only; then
  [[ -f "$audit_corpus" ]] || {
    echo "ecosystem: audit corpus not found: $audit_corpus" >&2
    exit 1
  }
  Rscript - "$audit_corpus" "$generated_dir" "${root_packages[@]}" <<'RS'
args <- commandArgs(trailingOnly = TRUE)
corpus_path <- args[[1]]
reports_dir <- args[[2]]
processed <- args[-c(1, 2)]
corpus <- jsonlite::fromJSON(corpus_path, simplifyDataFrame = TRUE)
audited <- intersect(processed, corpus$packages$name)

identity <- function(package, code, path, line, column) {
  sprintf("%s\t%s\t%s\t%s\t%s", package, code, sub("^\\./", "", path), line, column)
}
expected_rows <- corpus$findings[corpus$findings$package %in% audited, , drop = FALSE]
expected <- identity(
  expected_rows$package,
  expected_rows$code,
  expected_rows$path,
  expected_rows$line,
  expected_rows$column
)

actual <- character(0)
for (package in audited) {
  report <- file.path(reports_dir, paste0(package, ".root.txt"))
  lines <- readLines(report, encoding = "UTF-8", warn = FALSE)
  lines <- lines[nzchar(lines)]
  for (entry in lines) {
    fields <- regmatches(entry, regexec("^(.*):([0-9]+):([0-9]+) (RY[0-9]+)$", entry))[[1]]
    if (length(fields) != 5L) stop("malformed ecosystem report entry: ", entry)
    actual <- c(actual, identity(package, fields[[5]], fields[[2]], fields[[3]], fields[[4]]))
  }
}

multiset_delta <- function(left, right) {
  left_counts <- table(left)
  right_counts <- table(right)
  keys <- union(names(left_counts), names(right_counts))
  left_values <- left_counts[keys]
  right_values <- right_counts[keys]
  left_values[is.na(left_values)] <- 0L
  right_values[is.na(right_values)] <- 0L
  rep(keys, pmax(as.integer(left_values - right_values), 0L))
}
missing <- multiset_delta(expected, actual)
unowned <- multiset_delta(actual, expected)
required_rows <- expected_rows[
  which(expected_rows$label == "true_positive"),
  ,
  drop = FALSE
]
required <- identity(
  required_rows$package,
  required_rows$code,
  required_rows$path,
  required_rows$line,
  required_rows$column
)
missing_required <- multiset_delta(required, actual)

if (length(missing_required)) {
  writeLines(c("ecosystem: required reviewed findings disappeared:", paste0("  - ", missing_required)), stderr())
}
other_missing <- multiset_delta(missing, missing_required)
if (length(other_missing)) {
  writeLines(c("ecosystem: reviewed audit findings disappeared:", paste0("  - ", other_missing)), stderr())
}
if (length(unowned)) {
  writeLines(c("ecosystem: unowned hermetic findings appeared:", paste0("  - ", unowned)), stderr())
}
if (length(missing) || length(unowned)) {
  if (length(missing_required)) {
    writeLines("ecosystem: a reviewed true_positive finding disappeared; this gates the build in every mode.", stderr())
    quit(status = 1L)
  }
  reconciliation_mode <- if (!is.null(corpus$reconciliation)) corpus$reconciliation[[1L]] else "hermetic"
  if (reconciliation_mode == "audit-transcript") {
    writeLines(c(
      sprintf("ecosystem: %s is an audit transcript of an installed-library run;", corpus_path),
      "ecosystem: the hermetic-vs-audit delta above is reported for visibility and does not gate this build.",
      sprintf("ecosystem: to update the ledger, re-audit the %s corpus and regenerate it from the audit results (see docs/corpus/README.md).", corpus$corpus[[1L]])
    ), stderr())
  } else {
    writeLines(sprintf("ecosystem: update %s with the reviewed workstream delta", corpus_path), stderr())
    quit(status = 1L)
  }
}
RS
fi

summary_names=("SUMMARY.md")
if ! $local_only; then
  summary_names+=("SUMMARY.root.md")
fi

if ! $check; then
  for report_stem in "${processed_reports[@]}"; do
    cp "$generated_dir/$report_stem.txt" "$reports_dir/$report_stem.txt"
    cp "$generated_dir/$report_stem.full.txt" "$reports_dir/$report_stem.full.txt"
  done
  for summary_name in "${summary_names[@]}"; do
    cp "$generated_dir/$summary_name" "$reports_dir/$summary_name"
  done
  echo "ecosystem: updated reports for ${processed_packages[*]}"
  exit 0
fi

drift=0
for report_stem in "${processed_reports[@]}"; do
  for suffix in "" ".full"; do
    expected="$reports_dir/$report_stem$suffix.txt"
    actual="$generated_dir/$report_stem$suffix.txt"
    if [[ ! -f "$expected" ]] || ! cmp -s "$expected" "$actual"; then
      echo "ecosystem: report drift for $report_stem$suffix" >&2
      if [[ -f "$expected" ]]; then
        diff -u "$expected" "$actual" || true
      else
        diff -u /dev/null "$actual" || true
      fi
      drift=1
    fi
  done
done
for summary_name in "${summary_names[@]}"; do
  expected="$reports_dir/$summary_name"
  actual="$generated_dir/$summary_name"
  if [[ ! -f "$expected" ]] || ! cmp -s "$expected" "$actual"; then
    echo "ecosystem: report drift for $summary_name" >&2
    if [[ -f "$expected" ]]; then
      diff -u "$expected" "$actual" || true
    else
      diff -u /dev/null "$actual" || true
    fi
    drift=1
  fi
done

if ((drift)); then
  echo "ecosystem: regenerate reports with ecosystem/run.sh and commit them" >&2
  exit 1
fi
echo "ecosystem: committed reports are current"
