# Plan 14: Scope/flow modeling grab-bag

## Status: implemented (2026-07-12).

Five independent small fixes from ROADMAP items 8, 9, 10, 12 and amendment
F. Each is small; together they clear the remaining P1 clusters (processx
60, R6 37, evaluate/plyr/scales, matrixStats 342, crayon).

## Scope — files this plan may touch

- `crates/ry-checker/src/infer/misc.rs`
- `crates/ry-checker/src/infer/mod.rs`
- `crates/ry-checker/src/infer/call.rs`
- `crates/ry-checker/src/infer/index.rs`
- `crates/ry-checker/src/collect.rs`
- `crates/ry-checker/src/packages.rs`
- `crates/ry-checker/src/lib.rs`
- `crates/ry-cli/src/package_metadata.rs`
- `crates/ry-checker/src/tests.rs`
- `crates/ry-checker/testdata/`
- `crates/ry-core/src/ast.rs`, `crates/ry-core/src/parser.rs`,
  `crates/ry-core/tests/parser_correctness.rs` (Part E only: the parser
  currently lowers user infix `%op%` expressions to `Expr::Unknown`,
  discarding operands — represent them instead with operands preserved,
  e.g. as a call-shaped or dedicated infix node. Default checker behavior
  for an unrecognized `%op%`: infer both operands normally, return unknown.
  Make sure existing special-cased operators like the magrittr pipe are
  unaffected, and that DSL-ish operators do not start emitting new
  diagnostics in the existing test corpus — the full workspace suite is the
  guard.)

## Part A — `inherits()` type narrowing (item 9)

`extract_type_narrowing` in `infer/misc.rs` handles `is.numeric` etc. and a
generic `s3_predicate_target`. Add an arm for `inherits(x, "Cls")` (and its
`!inherits(...)` negation) narrowing `x` to carry class `"Cls"` on the
positive branch. The existing class-narrowing install path in
`apply_narrowing` should already accept it. Cluster: evaluate, plyr, scales.

## Part B — `useDynLib(.fixes=)` (item 8)

`package_metadata.rs::read_namespace` -> `packages::namespace_metadata`
does not model `useDynLib`. The native routine NAMES are not knowable
without parsing `src/`, so model the PREFIX:

- Parse `useDynLib(pkg, ..., .fixes = "prefix")` directives from NAMESPACE.
- Record the prefix(es) in the namespace metadata; during unbound-symbol
  checking, any symbol starting with a recorded prefix (with a non-empty
  remainder) resolves as an opaque binding.
- Plain `useDynLib(pkg)` without `.fixes` needs no new behavior.
- Cluster: processx (60 of 77 diags).

## Part C — dynamic-binding recognizers (item 12)

Best-effort recognizers, each ~10-20 lines, no soundness ambitions:

1. **R6 / S7 class bodies**: inside function literals that appear inside
   `R6Class(...)` / `new_class(...)` call arguments, bind `self`,
   `private`, and `super` as opaque. Also accept the
   `environment(f) <- value` replacement form anywhere without complaint
   (it is an ordinary replacement call; verify it does not currently
   error). Cluster: R6 (37), fastmap, memoise.
2. **Top-level `local({...})`**: first reproduce the actual FP shape —
   write a fixture from the processx/callr "standalone errors" idiom
   (a top-level `x <- local({ helper <- function() ...; main <- function()
   helper(); main })` where inner definitions reference each other) and
   whatever fails there, fix minimally. If nothing fails, check the
   prettyunits `eval(expression(...))` variant and cover that instead;
   report what you found either way.
3. **`assign("x", value, envir = asNamespace("pkg"))`** at top level:
   treat `x` as a defined binding for the rest of the file/package
   (crayon cluster).

## Part D — replacement-function schema plumbing (amendment F)

`dimnames(x) <- list(rn, cn)` desugaring: matrixStats had 342 diags, 90%
from this shape (also reshape). Investigate how replacement assignments
(`names<-`, `dimnames<-`, `colnames<-`, `rownames<-`, `attr<-`, `class<-`,
`levels<-`) are currently handled in assignment inference; ensure they
(a) never produce unbound/arg diagnostics for the target, (b) keep `x`
bound with its type updated opaquely (or with names, where cheap — e.g.
`names(x) <- c("a","b")` giving `x` a known-names schema is a nice-to-have,
implement only if the schema type supports it directly).

## Part E — `%<-%` destructuring assignment (zeallot / future)

The typeshed (plan 16) marks the `%<-%` LHS as quoted, which silences RY010
on the pattern itself, but the schema cannot express that `c(a, b) %<-% f()`
INTRODUCES bindings `a` and `b`. Add a checker-side recognizer: for a
top-level or body-level `lhs %<-% rhs` (and mirrored `rhs %->% lhs`) where
the destructure pattern is a bare symbol or a `c(...)`/`%<-%`-conventional
nesting of bare symbols, bind each symbol opaquely in the current scope.
Only when zeallot or future is attached/imported (check how `self.loaded` /
package context gating works for dplyr NSE and use the same gate).

## Part F — `vapply` regression guard (item 10 residue)

Add a regression test: `vapply(x, f, FUN.VALUE = character(1), USE.NAMES =
FALSE, extra = "chr")` — the `FUN.VALUE` template must be honored even when
`...` contains character arguments (readr FP). If the test fails, fix the
`higher_order` `fun_value_template` path minimally.

## Tests / acceptance

- Tests per part as described; testdata fixtures for the R6 and local()
  idioms.
- `cargo fmt`, `cargo clippy --workspace --all-targets`,
  `cargo test --workspace` all green.
- No git state-changing commands; preserve pre-existing uncommitted
  changes. Only touch Scope files.
