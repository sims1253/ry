# Plan 17: Replace wave-09-16 hardcodes with typeshed-derived semantics

## Status: implemented (2026-07-13). Repos: ry + ../r-typeshed.

Three semantics landed as Rust hardcodes that the typeshed can express
data-driven, continuing the plan-04 direction. After this plan the checker
consults stub metadata instead of name lists, so new packages get the same
treatment by adding stubs, not Rust.

## Scope

- ry: `crates/ry-checker/src/infer/call.rs`, `crates/ry-checker/src/infer/mod.rs`,
  `crates/ry-checker/src/nse.rs` (if the injection plumbing lives better there),
  `crates/ry-typeshed/src/lib.rs` (loader: new optional field),
  `crates/ry-checker/src/tests.rs`.
- r-typeshed: `schema/SCHEMA.md`, `stubs/withr/` (new), `stubs/r6/` or
  `stubs/R6/` (match existing stub dir naming conventions — check first),
  `stubs/s7/` (new, for `new_class`), `stubs/zeallot/`, `stubs/future/`.
- Do NOT rebuild `target/release` (a corpus run is using that binary);
  debug builds for tests are fine.

## Part A — `%<-%` gating via typeshed lookup

`infer/mod.rs` (~line 1321) gates `%<-%`/`%->%` destructuring on
`matches!(package, "zeallot" | "future")`. Replace with: the destructuring
recognizer activates when any loaded/imported package's typeshed stub
defines the operator (`%<-%` exists in `stubs/zeallot` and `stubs/future`
already). Keep behavior identical for the existing tests; the hardcoded
list disappears.

## Part B — `injects` stub metadata

Add an optional per-function field to the schema (document in SCHEMA.md):

```json
"injects": [
  {"into": ["code"], "strings_from": ["new"]},
  {"into": ["public", "private", "active"], "names": ["self", "private", "super"]}
]
```

Semantics: when a call to the function is inferred, each spec builds a
child scope for the arguments named in `into` (fall back to positional
match on the declared params): `strings_from` injects opaque bindings named
by string literals / injected-string values found in those source
arguments (exactly what `with_tempfile` does today via
`injected_string_bindings`); `names` injects the fixed identifiers as
opaque bindings — applied to function literals nested anywhere inside the
`into` arguments (the R6 case: methods in `public = list(...)`).

- Extend the strict serde loader in `crates/ry-typeshed/src/lib.rs`.
- Consume it in the checker where the `with_tempfile` and
  `R6Class`/`new_class` special cases live today (`infer/call.rs` ~69 and
  ~177), then DELETE those hardcoded arms.
- Author stubs: `withr` (at minimum `with_tempfile`, plus the obvious
  same-shape `with_file`, `with_tempdir`; only functions you are sure of),
  `R6::R6Class`, `S7::new_class` with the appropriate `injects` entries.
- `ry typeshed validate` must accept the new field (extend if it rejects
  unknown fields).

## Acceptance

- The three hardcoded sites are gone; all existing tests (including the
  withr/with_tempfile, R6, and zeallot/future destructuring tests from
  plans 09-16) still pass UNCHANGED — they now exercise the data path.
- New loader unit test for `injects` parsing; SCHEMA.md documents it.
- ry: `cargo fmt`, `cargo clippy --workspace --all-targets`,
  `cargo test --workspace` green. r-typeshed: stubs validate via
  `cargo run -p ry-cli -- typeshed validate ../r-typeshed/stubs` (debug).
- NOTE: the ry vendor copy (`crates/ry-typeshed/vendor/`) is synced by a
  separate script the orchestrator runs afterwards — but your ry tests load
  the VENDORED stubs, so for tests to pass you must ALSO mirror the new/
  changed stub files into `crates/ry-typeshed/vendor/` by hand (copy the
  files; do not run scripts/sync_typeshed.sh yourself, it rewrites SOURCE).
- No git state-changing commands in either repo; preserve all pre-existing
  uncommitted changes.
