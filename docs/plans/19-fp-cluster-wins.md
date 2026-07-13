# Plan 19: Five structural FP-cluster wins (post-corpus-measurement)

## Status: implemented (2026-07-13).

Corpus recount (2026-07-13, 303 packages, 10,992 diagnostics) shows five
mechanisms behind the top noisy packages. Each was verified against real
sites; every part lists its probe so you can confirm before/after with
`target/debug/ry` (packages live under /home/m0hawk/Documents/ry-audits/
packages/, read-only — never modify them; do NOT rebuild target/release).

## Part A — propagate `library()` from testthat helper/setup files

Verified: dtplyr/tests/testthat/helpers-library.R does `library(dplyr);
library(tidyr)`; our testthat context (ry-cli package_metadata) unions
helper top-level ASSIGNMENTS into test files but not their package
attachments, so all masked-verb columns in test files fire RY010
(dtplyr 334, dbplyr 347, tidyselect 336, dplyr 299, tidyr 239 are mostly
this). Fix in `crates/ry-cli/src/package_metadata.rs`:

1. When collecting helper bindings, ALSO collect top-level `library(pkg)` /
   `require(pkg)` calls from helper/setup files and add those packages to
   the attached set for every test file in the directory.
2. Check the helper filename globs: testthat loads ALL files whose name
   starts with `helper` or `setup` (dtplyr's file is `helpers-library.R`,
   note the plural). Widen the existing `helper-*`/`setup-*` matching to
   prefix matches `helper*.R` / `setup*.R` if it is stricter than that.

Probe: `dtplyr/tests/testthat/test-count.R` — `count(x)` sites must go
silent once dplyr/tidyr attach.

## Part B — data-drive the NSE mask gating from stub eval metadata

Verified repro: `library(rlist); r <- list.map(some_list(), . + score)`
still fires RY010 on `.` and `score` even though `stubs/rlist/rlist.json`
declares the expression parameter as data-masked — while the identical
mechanism works for dplyr/tidyr. Somewhere the mask machinery
(`crates/ry-checker/src/nse.rs` and its callers) is gated on a hardcoded
package set (dplyr/tidyverse) instead of "any loaded package whose stub
declares eval metadata". Find that gate and derive it from the typeshed:
if a called function resolves to a stub with `eval` modes and its package
is loaded/imported (same rules as dplyr today), honor the metadata.
Also make sure `.` (the rlist/pipe placeholder) inside a masked argument
is covered like any other symbol.

Probes: the repro above goes silent with `library(rlist)`; bit64's
`tests/testthat/test-integer64.R` patrick `with_parameters_test_that`
cases (`n2`, `n3`, ...) drop (bit64 321 -> ~30); rlist 213 -> ~20.

## Part C — derive NSE-ness of user-defined functions (defused parameters)

Verified: arrow/r/tests/testthat/helper-expectation.R:

```r
compare_dplyr_binding <- function(expr, tbl, warning = NA, ...) {
  expr <- rlang::enquo(expr)
  ...
}
```

812 of arrow's 1,195 diagnostics are `.input` inside `expr` arguments at
call sites of this helper. General rule to implement in the checker
(collect + call inference): when analyzing a user-defined function, mark a
parameter P as DEFUSED if P's first (or only) use in the body is as the
direct argument of a defusing function: `enquo`, `enquos`, `enexpr`,
`enexprs`, `ensym`, `ensyms`, `quo`, `substitute`, `match.call`,
`bquote`, `eval(substitute(...))` counts via `substitute`. (Both
`rlang::`-qualified and bare forms.) At call sites of that function,
arguments bound to defused parameters (positionally or by name, including
through `...` only if trivial — skip `...` if hard) are NOT inferred for
unbound-variable purposes: infer them as opaque without RY010, like a
masked argument with unknown schema. Precision guard: the marking pass
must be conservative — if P is also used normally before the defuse call,
do not mark it.

Probe: arrow `.input` count 812 -> ~0; total arrow 1,195 -> under 400.

## Part D — `foreach(...) %op% { ... }` loop-variable binding

Verified: caret/R/adaptive.R:37 uses
`foreach(iter = seq_along(...), parm = ..., .errorhandling = "stop") %op%
{ ... uses iter, parm ... }` where `%op%` is a LOCAL alias chosen between
`%do%`/`%dopar%`. 252 of caret's 370 diagnostics are `iter`/`parm`.
Recognizer (mirror the existing `%<-%` destructuring one in
`crates/ry-checker/src/infer/mod.rs`): for ANY user infix operator whose
LHS is a call to `foreach` (or a `%:%` chain of `foreach` calls), bind the
foreach call's NAMED arguments (excluding dot-prefixed control args like
`.errorhandling`, `.combine`, `.packages`, `.export`, ...) as opaque in
the RHS scope. Key on the LHS callee being `foreach`, not on the operator
name, since aliases are common. Gate on `foreach` being resolvable
(loaded/imported package or any stub defining it) OR simply on the callee
name — your call, note which.

Also add a minimal `foreach` stub in r-typeshed if none exists
(`foreach`, `%do%`, `%dopar%`, `%:%`) so the name resolves.

Probe: caret 370 -> ~120.

## Part E — `attach()` collapses unbound certainty + shiny testServer stub

1. Verified: MASS/inst/scripts/ch02.R does `attach(quine)` then uses
   column names (`Age`, `Sex`, ...) bare — 256 diagnostics, all this
   shape (urca, TTR similar). Base-R semantics: `attach(x)` inserts an
   environment with unknowable bindings on the search path. Implement:
   after a top-level or function-scope call to `attach(...)`, unresolved
   symbols in that scope (and nested scopes created after it) no longer
   emit RY010 (suppress entirely; the scope is unanalyzable). `detach()`
   need not restore precision (keep it simple, stay silent).
2. r-typeshed: add a `shiny` stub with `testServer` declaring the
   plan-17 `injects` metadata: names `session`, `input`, `output` into
   the app/expr arguments (check testServer's actual signature:
   `testServer(app, expr, args, session)` — inject into `expr`). Also
   `moduleServer(id, module)` if straightforward. Mirror new/changed stub
   files into `crates/ry-typeshed/vendor/` by hand (do not run
   scripts/sync_typeshed.sh).

Probes: MASS 256 -> ~30; shiny `session`/`output` clusters in
inst/app_template and tests drop substantially.

## Acceptance

- All probes above verified with a debug build and reported with
  before/after numbers per package (use
  `target/debug/ry check --statistics --exit-zero <pkg>` from
  /home/m0hawk/Documents/ry-audits/packages).
- New unit tests per part in `crates/ry-checker/src/tests.rs` /
  ry-cli tests (Part A), fixtures in testdata/ where natural.
- `cargo fmt`, `cargo clippy --workspace --all-targets`,
  `cargo test --workspace` all green; existing tests unchanged unless a
  behavior they assert was one of the FP classes above (say so if so).
- `cargo run -p ry-cli -- typeshed validate ../r-typeshed/stubs` green.
- No git state-changing commands in either repo; preserve all
  pre-existing uncommitted changes; do not rebuild target/release.
