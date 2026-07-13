# Plan 20: Residual FP fixes after plan 19

## Status: implemented (2026-07-13).

Follow-ups from the plan-19 probe results. Corpus packages for probing live
read-only at `/home/m0hawk/Documents/ry-audits/packages` — never modify
them, use `target/debug` builds only, never rebuild `target/release`.

## Part A — S3 methods inherit the NSE eval metadata of their generic

Verified failing probe: `dtplyr/tests/testthat/test-count.R:6` fires RY010
on `x` in `dt %>% count(x)` during full-package checking, while the
identical snippet checked standalone is silent.

Cause: the testthat context attaches dtplyr's namespace, so `count`
resolves to the package's own S3 methods (`count.dtplyr_step` etc.)
through the user-function table, and user-function call inference ignores
the typeshed eval/data-mask metadata that the dplyr `count` stub declares.

Fix (derivation, not hardcode): at call inference time
(`crates/ry-checker/src/infer/call.rs`), when the callee resolves to a
user-defined function AND the callee NAME matches a typeshed function
with `eval` metadata in a loaded/attached package (same gating as normal
stub NSE), apply that stub's eval modes to the arguments (mask/quote them)
while keeping the user function's return-type inference.

Probe: dtplyr total (284 now) drops to ~230 with the `test-count.R`
`x`-sites silent; dbplyr drops too. Unit test: a file defining
`count.mystep <- function(.data, ...) 1` (with `library(dplyr)`) where
`count(obj, some_col)` stays silent.

## Part B — type narrowing for expression-position `if`

Verified failing probe: `rlist/R/internal.R:149`
`uvalues <- if (is.function(proc)) proc(values) else values` fires
RY070 [high] ("`proc` is `union`, not a function").

`extract_type_narrowing` / `apply_narrowing`
(`crates/ry-checker/src/infer/misc.rs`) already handle `is.function` for
statement-level `if`; the expression-position `if` inference path (find
where an if-expression is inferred as a value in `infer/mod.rs`) does not
apply narrowing to its branch inference. Apply the same narrowing there.

Probe: the RY070 above disappears. Unit test:
`x <- if (is.function(f)) f(1) else f` produces no RY070.

## Part C — complete the rlist stub eval metadata

rlist still has ~120 diagnostics, mostly in its own tests
(`target/debug/ry check --exit-zero rlist` from the packages dir).
Measure which rlist functions' expression arguments are being checked
lexically (candidates: `list.class`, `list.table`, `list.iter`,
`list.first`/`list.last`, `list.which`, `list.findi`, `list.find`,
`list.count`, `list.remove.../list.exclude`, `list.clean`, `list.order`,
`list.mapv`, `list.all`/`list.any` — MEASURE, do not guess) and add
`data_mask` eval metadata for them to
`/home/m0hawk/Documents/r-typeshed/stubs/rlist/rlist.json`, mirroring the
changed file into `crates/ry-typeshed/vendor/rlist/`. Only mark parameters
rlist actually evaluates against list elements; when unsure, leave
unchecked.

Probe: rlist total (144 now) drops substantially.

## Acceptance

- Per-part before/after probe numbers in the final summary.
- `cargo fmt`, `cargo clippy --workspace --all-targets`,
  `cargo test --workspace` green.
- `cargo run -p ry-cli -- typeshed validate ../r-typeshed/stubs` green.
- Preserve all pre-existing uncommitted changes in both repos; no git
  state-changing commands; do not rebuild `target/release`.
