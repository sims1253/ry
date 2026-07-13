# Plan 11: NSE completion — rlang `{{ }}` embrace and the `.data` / `.env` pronouns

## Status: implemented (2026-07-12).

Derived from the top-300 CRAN audit (ROADMAP item 2, audited Partial). The
data-masking machinery exists (`crates/ry-checker/src/nse.rs`, eval-mode
metadata from the typeshed, `!!`/`!!!` stripping in `infer/mod.rs`). Two
gaps remain and both are common in real tidyverse package code.

## Scope — files this plan may touch

- `crates/ry-checker/src/nse.rs`
- `crates/ry-checker/src/infer/mod.rs`
- `crates/ry-checker/src/infer/index.rs`
- `crates/ry-checker/src/tests.rs`
- `crates/ry-checker/testdata/` (new fixtures)

## Part A — `{{ x }}` (rlang embrace)

The parser today emits `{{ x }}` as nested braced blocks / a braced call.
Inside a function body, `{{ x }}` forwards the caller's expression for
formal `x`; `x` itself IS a formal of the enclosing function, so plain
scope lookup usually succeeds — verify what actually happens today with a
failing-or-passing test first:

```r
my_summarise <- function(df, group_var) {
  dplyr::summarise(df, mean = mean({{ group_var }}))
}
```

Requirements:
1. `{{ x }}` where `x` resolves in scope: no diagnostic, result opaque.
2. `{{ x }}` must be recognized structurally (a block containing exactly a
   block containing exactly one symbol) and treated as an NSE escape —
   i.e. even when it appears in a masked argument, the inner symbol is
   looked up in the FUNCTION scope, not treated as a data column; and the
   double-brace shape must never produce "empty block"/type noise.
3. `{{ x }}` where `x` does NOT resolve in function scope: keep RY010 (it
   is a genuine typo — this preserves precision).

## Part B — `.data$col`, `.data[["col"]]`, `.env$var`

Inside a data-masked argument (the mask scope machinery in `nse.rs` —
see `scope_with_columns` and the `dplyr_data_mask` handling):

1. `.data` itself must never fire RY010 inside a mask context.
2. `.data$col` / `.data[["col"]]`: when the active mask has a known column
   schema, resolve the column — keep the existing RY060-style unknown-column
   diagnostic if the column is absent from a KNOWN schema; when the schema
   is unknown/opaque, return opaque silently.
3. `.env$var` / `.env[["var"]]`: resolve `var` against the enclosing
   function scope (normal lookup, RY010 if unbound).
4. Outside any mask context, `.data`/`.env` keep current behavior (they are
   rlang exports; if the package imports rlang they resolve, otherwise
   normal RY010 applies).

Check `infer/index.rs` `$`/`[[` handling for where receiver-name special
cases belong; RY060 schema checking for `df$col` already exists there as a
pattern to follow.

## Tests / acceptance

- Unit tests: embrace with bound formal (silent), embrace with typo'd inner
  symbol (RY010), `.data$known_col` against a literal-schema mask (silent),
  `.data$missing_col` against a known schema (diagnostic), `.data$col` with
  opaque mask (silent), `.env$bound` (silent), `.env$unbound` (RY010),
  `.data` bare inside `mutate` (silent).
- One `testdata/` fixture with the `my_summarise` embrace pattern above.
- `cargo fmt`, `cargo clippy --workspace --all-targets`,
  `cargo test --workspace` all green.
- No git state-changing commands; preserve pre-existing uncommitted changes.
- Only touch the files in Scope; note anything else in your final message.
