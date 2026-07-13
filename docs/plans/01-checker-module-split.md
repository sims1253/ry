# Plan 01: Split ry-checker/src/lib.rs into modules

## Status: ready for implementation
## Repo: ry (this repo). Branch off `dev`.

## Context

`crates/ry-checker/src/lib.rs` is ~9,400 lines: ~6,600 lines of implementation
and ~2,800 lines of inline `#[cfg(test)]` tests (test module starts around line
6590). Everything — scope handling, S3 dispatch, NSE verbs, higher-order
function modeling, type inference for calls/binops/indexing/pipes, suppression
comment parsing — lives in one file. This blocks review, contribution, and the
follow-up refactors in plans 04/05.

This is a **pure mechanical refactor**: no behavior change, no public API
change, no diagnostic output change.

## Target module layout

Split `lib.rs` into (keep `pub use` re-exports in `lib.rs` so the public API
is unchanged):

- `lib.rs` — crate docs, module declarations, public re-exports, the
  `Checker` struct definition and its constructor/entry points (`new`,
  `check`, `check_with_scope`, `take_diagnostics`, the `set_*` methods,
  `emit`, `apply_filter`). Keep this file under ~800 lines.
- `scope.rs` — `Scope`, `ReturnSlots`, `FnTable` and binding helpers
  (`binding_name`, `ident_name`, ambient globals table + `ambient_global_type`).
- `s3.rs` — `S3_GENERICS`, `S3_DENYLIST`, `split_s3_method_name`,
  `split_s3_operator_method_name`, S3 dispatch helpers
  (`try_s3_binop_dispatch`, `try_s3_unary_dispatch`), external S3 method
  handling.
- `nse.rs` — `NseVerb` and every `infer_nse_*` method, `infer_tidyselect_expr`,
  `dplyr_data_mask_scope`, `scope_with_columns`, `infer_dplyr_join`,
  `infer_tidyr_pivot_call`.
- `higher_order.rs` — `HigherOrderFunc`, `infer_higher_order_call`,
  `infer_ho_result`, `extract_callback`, `unwrap_in_parallel`, all `ho_*`
  methods.
- `collect.rs` — pass-1 collection: `collect_fns*`, `collect_declared_globals_*`,
  `collect_forwarded_calls`, `collect_nested_fns_*`, `record_fn`,
  `ForwardedCall`.
- `fixpoint.rs` — `refine_fn_return`, `run_fixpoint`, `MAX_FIXPOINT_DEPTH`,
  `MAX_CLOSURE_DEPTH`, closure signature building
  (`function_value_from_literal`, `build_function_signature*`,
  `trailing_return_type`).
- `infer/mod.rs` — `infer`, `infer_discarding`, `infer_block_expr`,
  `infer_stmt_value`, `walk_stmt`, `check_stmt`, `merge_branch_bindings`,
  assignment targets (`assign_target`, `assign_index_target`,
  `assign_class_attribute`, `assign_nested_record_path`).
- `infer/call.rs` — `infer_call`, `resolve_typeshed_sig`, `apply_sig`,
  `has_function_anywhere`, `infer_structure_call`, `infer_switch_call`,
  `infer_trycatch_call`, `infer_rep`, `infer_seq`,
  `diagnostic_parameter_type`, `forwarded_default_type`.
- `infer/binop.rs` — `infer_binop`, `infer_short_circuit_binop`,
  `non_divisible_recycling`.
- `infer/pipe.rs` — `infer_pipe`, `infer_pipe_tee`, `infer_if_expr` (if it
  is entangled with pipe code keep it in `infer/mod.rs` instead).
- `infer/index.rs` — `infer_index`, `assigned_column_name`,
  `type_with_assigned_column`, `emit_undefined_column`, column schema
  helpers.
- `suppress.rs` — suppression comment parsing (`ry: ignore`, `noqa`, file
  level markers) and `SeverityFilter` application helpers, if these live in
  lib.rs today (`apply_filter_to_diagnostics` etc. — check `diagnostics.rs`
  first; do not duplicate).

Existing separate files (`diagnostics.rs`, `format.rs`, `packages.rs`,
`project.rs`, `rules.rs`) stay as they are.

## Test placement

Move the inline `#[cfg(test)]` tests out of lib.rs into the module that owns
the code under test (e.g. NSE tests into `nse.rs`'s test module, suppression
tests into `suppress.rs`). Tests that exercise the whole checker end-to-end
(the `fn check(src) -> Vec<Diagnostic>` helper style) can go to
`crates/ry-checker/tests/checker_integration.rs` OR into a
`#[cfg(test)] mod tests` in `infer/mod.rs` — prefer whichever needs the
fewest visibility changes. The shared `fn check(...)`/`check_with_scope(...)`
test helpers should live in one place (e.g. a `#[cfg(test)] pub(crate) mod
test_util` in lib.rs) and be reused.

## Rules

1. NO behavior changes. This must be a move-only refactor. If you find a bug,
   leave a `// TODO` comment, do not fix it here.
2. Prefer `pub(crate)` over `pub` for anything newly exposed between modules.
3. Public API of the crate (what `ry-cli`, `ry-lsp`, and the tests/ dir use)
   must not change: `cargo build --workspace` must pass without touching other
   crates. If a small visibility bump is unavoidable, keep it minimal.
4. Methods on `Checker` can be split across multiple `impl Checker` blocks in
   the new module files — that is the intended mechanism.
5. Keep doc comments with the items they document.
6. Do not reformat unrelated code; run `cargo fmt` at the end.
7. Do NOT run any git commands (no commits, no branches). Leave all changes
   in the working tree.

## Acceptance criteria

- `cargo build --workspace` clean.
- `cargo test --workspace` passes with the same test count as before the
  refactor (count tests before starting: `cargo test -p ry-checker 2>&1 |
  tail`, record the numbers, compare after).
- `cargo clippy --workspace --all-targets` introduces no NEW warnings.
- `crates/ry-checker/src/lib.rs` is under ~800 lines.
- No file under `crates/ry-checker/src/` exceeds ~1,500 lines (except
  generated/test-heavy files if unavoidable).
- `cargo fmt --check` passes.
