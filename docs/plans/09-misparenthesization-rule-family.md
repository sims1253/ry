# Plan 09: Mis-parenthesization rule family + `hasArg()` lint

## Status: implemented (2026-07-12).

Derived from the top-300 CRAN audit (ry-audits ROADMAP items 13, C, D).
At least 12 confirmed real bugs across the corpus fall in this family;
several are invisible to ry today (gtable, ModelMetrics, openxlsx, psych,
diffobj, quantreg).

## Scope — files this plan may touch

- `crates/ry-checker/src/infer/call.rs`
- `crates/ry-checker/src/infer/binop.rs`
- `crates/ry-checker/src/infer/mod.rs` (AST-level dispatch: RY095 needs the
  raw `Expr::BinOp` operand structure; RY096 needs the function-body entry
  point to track enclosing formals)
- `crates/ry-checker/src/lib.rs` (only if checker state must carry the
  enclosing-formals stack)
- `crates/ry-checker/src/rules.rs`
- `crates/ry-checker/src/tests.rs`
- `crates/ry-checker/testdata/` (new fixtures)

Do NOT touch other files. If you believe another file must change, stop and
note it in your final message instead of editing it.

## Part A — generalize RY093 beyond `length()`

Current state: `infer/call.rs` (search for `RY093`) fires when a relational
`BinOp` (`< <= > >= == !=`) is the **direct first argument** of `length()`.

Extend the callee set to `{"length", "nchar", "abs"}`.

- Confirmed real bugs: `nchar(formatCodes > 3)` (openxlsx, x3),
  `abs(dv.cors[i,k] > e.cut)` (psych), `length(actual > 10000)` (ModelMetrics).
- **Deliberately EXCLUDE `sum()`**: `sum(x > 0)` is the idiomatic R way to
  count matches; flagging it would be a false-positive firehose. Document
  this exclusion in a code comment.
- Keep message shape: "comparison is inside `nchar()`; ..." — use the actual
  callee name in the message.

## Part B — RY093 must fire inside `&&`/`||` operands (audit item 13b)

Today the scan only sees calls reached during normal inference of `if`/
`while` conditions; the gtable bug `length(...) == x || q` shape was missed
because the audit found RY093 checks do not run for expressions nested in
short-circuit operators in all positions. Verify with a test that
`if (length(x == y) || q) ...` AND standalone `stopifnot(length(x == y) && z)`
each produce RY093. If the RHS of `&&`/`||` is inferred against a cloned
branch scope (see `infer_short_circuit_binop` in `infer/binop.rs`), make sure
diagnostics emitted there still reach the main sink. Fix whatever gap the
failing test exposes; if there is no gap, keep the regression tests.

## Part C — new rule RY095: `!f(x) == y` precedence lint

`!` binds tighter than `==`, so `!all(diff(x)) == 1L` parses as
`(!all(diff(x))) == 1L` — a confirmed real bug (diffobj).

- Trigger: `BinOp` with op `Eq` or `Ne` whose **LHS is a unary `!`
  expression** and whose **RHS is a numeric or string literal**.
- Do NOT fire when the RHS is `TRUE`/`FALSE`/logical (comparing a logical to
  a logical literal is redundant but not a precedence bug) or another
  expression (too noisy).
- Rule registration in `rules.rs`:
  code `RY095`, name `negation-comparison-precedence`,
  default severity Warning,
  summary "`!x == y` parses as `(!x) == y`; use `!(x == y)` or `x != y`."

## Part D — new rule RY096: `hasArg()` on a non-formal

quantreg `logLik.rq` real bug: `if (!hasArg(edfThresh)) edfThresh <- 1e-4`
where `edfThresh` is not a formal — passing it via `...` makes `hasArg`
return TRUE while the name stays unbound in the body.

- Trigger: a call to `hasArg(name)` (bare symbol or string literal argument)
  lexically inside a function whose formals do NOT include `name`.
- Fire regardless of whether the function has `...` (without `...` the check
  is constant-FALSE, which is also a bug; with `...` it is the quantreg trap).
  Mention the `...` case in the message when `...` is present.
- Rule registration: code `RY096`, name `hasarg-non-formal`, default severity
  Warning, summary "`hasArg()` names a parameter that is not a formal of the
  enclosing function."
- You will need access to the enclosing function's formals at the call site;
  check how the inferencer tracks the current function (e.g. what
  `infer_function`/dispatch context already stores) and thread the formal
  list through if it is not already available.

## Tests / acceptance

- Unit tests in `tests.rs` for: each new callee in Part A (positive +
  negative: `sum(x > 0)` stays silent, `length(x) > 0` stays silent);
  Part B `&&`/`||` shapes; RY095 positive (`!all(x) == 1L`) and negatives
  (`!x`, `(!x) == TRUE`, `!(a == b)`); RY096 positive and negative (name IS
  a formal; `hasArg` called at top level outside any function stays silent).
- Add at least one `testdata/` fixture mirroring a real-corpus shape
  (e.g. the diffobj and quantreg patterns).
- `cargo fmt`, `cargo clippy --workspace --all-targets`, and
  `cargo test --workspace` must all pass.
- Do not run any git state-changing commands; leave changes in the working
  tree. There are pre-existing uncommitted changes — preserve them.
