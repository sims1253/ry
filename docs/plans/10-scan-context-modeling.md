# Plan 10: Package-aware scan contexts — the "model" half

## Status: implemented (2026-07-12).

Derived from the top-300 CRAN audit (ry-audits ROADMAP item 1). The "skip"
half already landed (`crates/ry-cli/src/main.rs`
`is_excluded_package_directory`: revdep/, src/, tests/testthat/_snaps/).
This plan implements the "model" half: 87% of audit diagnostics land outside
`R/`, and most are the same root causes — tests must be modeled correctly,
not skipped (real bugs live there, e.g. sf's missing `print.WKB`).

## Scope — files this plan may touch

- `crates/ry-cli/src/main.rs`
- `crates/ry-cli/src/package_metadata.rs`
- `crates/ry-cli/src/config.rs`
- `crates/ry-checker/src/rules.rs` (ONLY to append the RY097 rule entry)
- `crates/ry-checker/src/lib.rs` / `crates/ry-checker/src/project.rs` only
  if strictly needed to plumb preloaded bindings — keep such edits minimal.
- New testdata under `crates/ry-cli/` test fixtures if that is where scan
  tests live today (check existing `package_scan_*` tests in main.rs).

## Part A — testthat evaluation context

When a checked file lives under `tests/testthat/` of a package root
(DESCRIPTION present):

1. The package's own namespace is attached: internal functions defined in
   the package's `R/` files must be resolvable. Check what project mode
   (`project.rs`) already collects for `R/` and reuse that mechanism —
   test files should see the same binding set as intra-package code, not
   just NAMESPACE exports.
2. `testthat` plus the DESCRIPTION `Depends` AND `Suggests` packages are
   treated as attached (their typeshed/ambient symbols resolvable) for
   these files. `package_metadata::resolve` already parses DESCRIPTION —
   extend what it attaches per-directory.
3. `helper-*.R` and `setup-*.R` files in `tests/testthat/` load before test
   files: parse them and union their top-level assignments (and function
   definitions) into the binding set visible to every `test-*.R` file in
   the same directory. Helpers must also be checked themselves (they are
   code), but their bindings leak forward.

## Part B — `.Rbuildignore` as an exclusion signal

- `.Rbuildignore` lines are **Perl-compatible regexes** matched against
  package-relative paths (this is R's semantics — NOT globs). Empty lines
  and `#` comments are skipped.
- In the recursive file collection, exclude files whose relative path
  matches any pattern — EXCEPT never exclude paths under `tests/` or `R/`
  via this signal (many repos .Rbuildignore their tests but still want them
  checked; the audit calls this out explicitly).
- Use a conservative regex crate already in the dependency tree if
  possible; if `.Rbuildignore` contains a pattern the regex engine cannot
  compile, skip that pattern silently.

## Part C — "not an R file" detection (RY097)

Hmisc and quantreg ship Fortran-dialect Ratfor `.r` files; quantreg alone
produced 2,035 garbage diagnostics from them. `src/` is already excluded,
but the content-level guard must exist everywhere:

- After parsing a file, if MORE THAN HALF of its top-level statements are
  parse errors (pick the concrete signal from what the parser exposes —
  error nodes, failed statements), drop ALL diagnostics for that file and
  emit a single info-severity RY097 diagnostic at 1:1.
- Register in `rules.rs`: code `RY097`, name `not-r-source`, default
  severity Info (use the crate's lowest severity — check the `Severity`
  enum), summary "File does not appear to be R source; diagnostics
  suppressed." Append the entry; do not reorder existing entries (plan 09
  added RY095/RY096 in an earlier batch — if they are present, append after
  them).
- This must also fire for intentionally-broken test fixtures (pkgload) —
  one info line instead of hundreds of errors is the desired behavior.

## Part D — interactive contexts

For files under `data-raw/`, `demo/`, and `vignettes/` of a package root:
attach the DESCRIPTION `Depends` packages (interactive context). Do NOT
implement confidence demotion here — that is plan 12's job; just make the
attachment context right so rvest's `keydef` typo class stays detectable.

## Tests / acceptance

- Tests colocated with the existing `package_scan_*` tests: testthat file
  resolving (a) an internal function from `R/`, (b) a helper-file binding,
  (c) a Suggests package symbol; `.Rbuildignore` excluding a matching file
  but never `tests/`/`R/`; a majority-invalid `.r` fixture yielding exactly
  one RY097 and nothing else; `data-raw/` file resolving a Depends symbol.
- `cargo fmt`, `cargo clippy --workspace --all-targets`,
  `cargo test --workspace` all green.
- No git state-changing commands; preserve pre-existing uncommitted changes.
