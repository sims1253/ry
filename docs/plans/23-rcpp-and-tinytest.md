# Plan 23: Rcpp as a first-class package + tinytest scan context

## Status: ready. Repos: ry (this repo) + ../r-typeshed.

Rcpp is at 224 corpus diagnostics (all RY010): 220 under `inst/tinytest/`
(functions injected by `Rcpp::sourceCpp("cpp/*.cpp")`), 4 in `R/Module.R`
(native-symbol internals, below the chase threshold). Probe read-only at
/home/m0hawk/Documents/ry-audits/packages with target/debug; never
rebuild target/release; never modify ry-audits.

## Part A — `scope_effect` stub metadata + Rcpp stub

The plan-19 `attach()` fix added `Scope::mark_search_path_unknown()`.
Make that mechanism data-driven:

1. New optional per-function stub field `"scope_effect":
   "unknown_bindings"` (document in r-typeshed schema/SCHEMA.md; extend
   the strict loader in `crates/ry-typeshed/src/lib.rs`; the name must
   not collide with the existing `schema_effect` semantics).
2. Checker: when a call resolves to a stub with that effect, call the
   same mark on the current scope that `attach()` uses today. Then
   REPLACE the hardcoded `attach` recognizer with `scope_effect` metadata
   on `base::attach` in `stubs/base/base.json` (keep behavior identical;
   the base generator must not strip the hand-added field — verify
   `gen_standard_globals.R --check` still passes, and if the generator
   owns that file's function entries, put attach's entry wherever
   hand-curated entries live).
3. New `stubs/rcpp/` (match existing stub dir naming) for package `Rcpp`:
   `sourceCpp` (scope_effect unknown_bindings, opaque return),
   `cppFunction` (returns an opaque function), `evalCpp` (opaque),
   `loadModule`, `setRcppClass` (opaque; if setRcppClass obviously
   introduces the named class binding, model via existing machinery,
   else leave opaque). Mirror into `crates/ry-typeshed/vendor/`.

## Part B — tinytest scan context

Mirror the testthat context (plan 10, `crates/ry-cli/src/
package_metadata.rs`) for `inst/tinytest/` files of a package root:

- The package's own namespace is attached (internal functions visible).
- `tinytest` is attached: add a minimal `stubs/tinytest/` stub with its
  exported `expect_*` family, `run_test_file`, `run_test_dir`, `at_home`,
  `exit_file` (check `getNamespaceExports("tinytest")` if installed;
  otherwise model the documented core set).
- DESCRIPTION Depends/Suggests attach, same as testthat.

## Part C — probes

- Rcpp: 224 -> target under 25.
- Also report before/after for TH.data (112), units (100), and TTR (184)
  — they may use tinytest and drop for free; do not chase their residues
  otherwise.

## Acceptance

- Unit tests: sourceCpp call silences later unbound reads in the same
  scope; a tinytest file resolving a package-internal function; loader
  test for `scope_effect`.
- `cargo fmt`, `cargo clippy --workspace --all-targets`,
  `cargo test --workspace` green; typeshed validate green
  (`cargo run -p ry-cli -- typeshed validate ../r-typeshed/stubs`).
- Preserve all pre-existing uncommitted changes in both repos; no git
  state-changing commands.
