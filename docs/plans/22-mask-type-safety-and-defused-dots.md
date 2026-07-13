# Plan 22: Mask-shadowing type safety + defused `...` derivation

## Status: implemented (2026-07-13).

Final two tidyverse residual mechanisms (dplyr 143 -> target well under
100). Probe read-only at /home/m0hawk/Documents/ry-audits/packages with
target/debug; never rebuild target/release; never modify ry-audits.

## Part A — no type-based diagnostics for lexically-resolved symbols in unknown-schema masks

Verified: dplyr/tests/testthat/test-across.R:494

```r
df |> mutate(data.frame(x = x / y, y = y / y, z = z / y))
```

fires RY040 [low] "cannot apply arithmetic op to `list` and `character`"
because `x`/`y`/`z` resolve to LEXICAL test-file bindings of those types.
In R, data-mask columns shadow lexical bindings; when the mask's schema
is UNKNOWN we cannot know whether the lexical binding is what the code
sees, so its TYPE must not drive diagnostics.

Fix: inside a data-masked argument whose mask schema is unknown, a symbol
that resolves lexically infers as OPAQUE (unknown type) instead of its
lexical type. Unbound symbols keep their current (suppressed/RY010-free)
behavior; symbols in masks with KNOWN schemas keep full checking. This
must kill type-family diagnostics (RY040/RY030/RY033/...) sourced from
lexical bindings under unknown masks without changing anything outside
masks.

Probe: dplyr RY040 count (53 now, mostly test-across.R) drops to ~0.
Unit test: lexical `y <- "a"` + `mutate(df, x = x / y)` on unknown `df`
is silent; the same expression OUTSIDE a mask still errors.

## Part B — extend defused-parameter derivation to `...`

Verified: dplyr/tests/testthat/test-colwise-filter.R:68 —
`all_exprs(am == 1)` fires RY010 on `am`. `all_exprs` is a dplyr
internal (visible via the testthat namespace attach) whose body defuses
its arguments with `enquos(...)`. The plan-19 defused-parameter
derivation deliberately skipped `...`.

Fix: when a user function's `...` is used EXCLUSIVELY as the direct
argument of defusing calls (`enquos`, `enexprs`, `ensyms`, `quos`,
`exprs`, `match.call`, `substitute`) in its body, mark `...` as defused:
call-site arguments that land in `...` (i.e. not matched to named
formals) are not lexically checked. If `...` is ALSO used normally
anywhere (e.g. `list(...)`, `c(...)`, forwarded to another call), do NOT
mark it — conservative like plan 19.

Probe: the test-colwise-filter.R `am`/`cyl` sites go silent; dplyr RY010
(62 now) drops noticeably. Unit test: `f <- function(...) enquos(...)` +
`f(not_a_binding == 1)` silent; `g <- function(...) sum(...)` +
`g(not_a_binding)` still fires.

## Scope

- `crates/ry-checker/src/nse.rs`, `crates/ry-checker/src/infer/` (mask
  inference path; defused-param derivation in collect.rs), `tests.rs`,
  testdata.

## Acceptance

- Before/after numbers for dplyr, dtplyr, dbplyr, tidyr, tidyselect
  (before: 143/9/57/53/20) — the other four must NOT regress.
- `cargo fmt`, `cargo clippy --workspace --all-targets`,
  `cargo test --workspace` green.
- Preserve pre-existing uncommitted changes; no git state-changing
  commands.
