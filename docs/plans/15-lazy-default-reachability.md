# Plan 15: Lazy-default force-order analysis (amendment E, second half)

## Status: implemented (2026-07-12).

Background: default arguments referencing body-locals
(`function(x, ylim = range(Px)) { ...; Px <- compute(x); plot(ylim) }`)
are idiomatic and CORRECT R — lattice alone produced 240 FPs, and an
uncommitted change already silences the whole class by letting defaults
resolve against all body-assigned names (see `infer/mod.rs`, the
lazy-default handling around the body-local pre-pass).

But mnormt's `dmt()` shows the same syntax hiding a real bug: an early
`return()` path forces the defaulted parameter BEFORE the body statement
that assigns the referenced local has run. Result: ry currently stays
silent on both the idiom AND the bug. This plan adds the discriminator.

## Scope — files this plan may touch

- `crates/ry-checker/src/infer/mod.rs`
- `crates/ry-checker/src/infer/misc.rs` (helpers if needed)
- `crates/ry-checker/src/rules.rs` (one new rule entry, appended)
- `crates/ry-checker/src/tests.rs`
- `crates/ry-checker/testdata/`

## Design

For each function whose default expressions reference body-locals, run a
single linear pass over the body's top-level statements tracking two
events per (parameter P with default referencing local L):

- **force(P)**: the first statement that USES P — anywhere in the
  statement, including inside `if` conditions and inside sub-expressions.
  (Lazy evaluation: P's default runs at first use.)
- **assign(L)**: the statement whose top-level effect assigns L (plain
  `L <- ...` at body level; assignments nested in `if` branches count as
  NOT guaranteed).

Flag when force(P) can happen before assign(L) is guaranteed:

1. force(P) occurs at an earlier statement index than any top-level
   assign(L), OR
2. force(P) occurs inside an `if` branch that can `return()`/`stop()` (or
   simply inside the condition) at an earlier index than assign(L).

Stay SILENT when the only uses of P come after a top-level assign(L) —
that is the lattice idiom. When in doubt (P used inside a closure, L
assigned conditionally in all observed uses' dominators, anything you
cannot cheaply prove), stay silent. False positives here would undo the
240-FP win; the audit's mnormt case is the only confirmed TP, so precision
is everything.

The mnormt shape (distilled acceptance test):

```r
dmt <- function(x, mean = rep(0, d), S, df = Inf) {
  if (df == Inf) return(dmnorm(x, mean, S))  # forces `mean`; `d` unset!
  d <- ncol(S)
  ...
}
```

Rule registration: append to `rules.rs` — code `RY098`, name
`default-forced-before-assignment`, default severity Warning, summary
"A parameter default references a body-local that may not be assigned yet
on some execution path."

## Tests / acceptance

- Positive: the mnormt shape above fires RY098 on the `return(...)` use.
- Negatives (all silent): the lattice shape (single tail use after
  assignment); default referencing another PARAMETER; local assigned
  before any use but inside the same statement chain; P never used; P used
  only inside a function literal defined early but called late.
- `cargo fmt`, `cargo clippy --workspace --all-targets`,
  `cargo test --workspace` all green.
- No git state-changing commands; preserve pre-existing uncommitted
  changes. Only touch Scope files.
