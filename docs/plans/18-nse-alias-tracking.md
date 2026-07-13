# Plan 18: Call-semantics follow function aliases

## Status: implemented (2026-07-13).

Regression found in the ecosystem snapshot after plan 14: cli's
`R/time-ago.R` does

```r
e <- expression
vague_dt_default <- list(
  list(c = e(seconds < 10), s = "moments ago"),
  list(c = e(minutes < 45), s = e("%d minutes ago" %s% round(minutes))),
  ...
)
```

ry special-cases quoting functions (`expression`, `quote`, `vars`, `~`,
`substitute`, `bquote`, and typeshed `eval`-mode metadata) by CALLEE NAME
only, so the alias `e` gets none of that treatment. This was previously
masked because `%op%` operands were discarded by the parser; since plan 14
preserves them, `e(... %s% round(minutes))` now infers the quoted operands
and emits 16 fresh RY010s in cli alone (see
`git diff ecosystem/reports/cli.txt`).

## Fix — alias resolution at call sites

1. When inferring a simple assignment whose RHS is a BARE IDENT that names
   a function — one of the hardcoded NSE/quoting special cases, a typeshed
   function (any package in scope), or an ambient function — record on the
   new binding the ultimate target name (follow existing alias chains;
   cap depth to avoid cycles).
2. At call inference time, resolve the callee name through that alias
   metadata FIRST, so every downstream special case (NSE quoting, typeshed
   signature + eval modes, printf checks, etc.) sees the target name.
   The existing `lookup_name` computation in `infer/call.rs` is the choke
   point.
3. Only bare-ident RHS (`e <- expression`). Do NOT attempt closures,
   `match.fun`, or computed functions. Re-assignment of the alias to a
   non-function or another function must update/clear the metadata
   (last write wins, as the scope already behaves).

## Scope

- `crates/ry-checker/src/lib.rs` (binding metadata)
- `crates/ry-checker/src/infer/mod.rs` (assignment site)
- `crates/ry-checker/src/infer/call.rs` (callee resolution)
- `crates/ry-checker/src/tests.rs`, `crates/ry-checker/testdata/`

## Tests / acceptance

- Fixture distilled from cli time-ago (alias of `expression` + user infix
  inside it): zero diagnostics.
- `q <- quote; q(undefined_sym)` silent; `s <- sprintf; s("%d %d", 1)`
  fires RY094 through the alias; alias overwritten by a local function
  loses the special semantics.
- Run `RY_BINARY=target/debug/ry bash ecosystem/run.sh --local` if helpful,
  but the authoritative check the orchestrator will run afterwards is the
  full ecosystem regeneration — your job is that
  `cargo test --workspace`, `cargo fmt`, `cargo clippy --workspace
  --all-targets` are green and the new tests pass.
- Do NOT rebuild target/release. No git state-changing commands; preserve
  all pre-existing uncommitted changes.
