# Parser `Option` propagation audit (0.9)

The audit requires every `?`, `.ok()?`, and `None` path in the production R
parser to have an owner. This audit covers `crates/ry-core/src/parser.rs` from
`RParser::new` through `namespace_op` (tests and the byte-column utility are not
parser lowering). It was performed against the production parser at the 0.9 release candidate.

The inventory contains **56 `?` operators on 45 source lines**, including the
single `.ok()?`, and every production `None` return/construction/match arm.
Sites are grouped below by the contract that owns them. There are no unowned
sites.

Status meanings:

- **safe** — cannot erase a parse-clean R statement; or it is non-AST error
  propagation;
- **explicit** — malformed input may stop lowering, but tree-sitter necessarily
  marks it and `collect_parse_errors` produces an explicit RY000 diagnostic;
- **fixed** — the old path could silently erase valid syntax and was made total.

## `?` inventory

| Site(s) | Status | Ownership argument |
| --- | --- | --- |
| `RParser::new` (`set_language`) and `parse` / `parse_with_tree` (`parse_with_tree`, tree creation) | safe | These propagate `Result<_, ParseError>`, not `Option` from AST lowering. No successful `SourceFile` is partially deleted. |
| `lower_stmt`: `lower_binary`, `lower_call`, identifier `text` | explicit | For parse-clean nodes the grammar supplies the required fields and node text comes from the same UTF-8 `&str`. Missing fields occur only on recovered syntax, for which `collect_parse_errors` owns the omission. R6 exercises all fixture-derived statement forms. |
| `try_lower_assign`: operator/text, lhs/rhs, and four operand lowerings | safe / explicit | `None` is deliberately a tri-state “not an assignment” result and falls through to `lower_binary`. Missing grammar fields or failed child lowering are recovered syntax and produce RY000. Parse-clean numeric children are total after the integer/float fixes below. |
| `lower_if` and `lower_if_expr`: condition, consequence, branch lowering | explicit | `condition` and `consequence` are required grammar fields. A missing/invalid required child is recovered syntax with RY000. An absent alternative is valid R and is separately represented by `else_: None`. |
| `lower_for`: variable/text, sequence, body | explicit | All are required by the grammar. Failure therefore accompanies an ERROR/MISSING node and RY000. |
| `lower_function_def_as_stmt` and `lower_function_literal`: parameters and body | explicit | Both fields are grammar-required; malformed forms are diagnosed. |
| `lower_expr`: identifier, integer, float, string, and NA `text` | safe | `Node::utf8_text` reads a tree-sitter node range from the exact UTF-8 source that produced the tree. Valid token ranges are UTF-8 boundaries. Integer and float *numeric conversion* no longer uses `?`. |
| `lower_call` and `lower_index`: function/base child | explicit | The function field is grammar-required; malformed recovered calls/indexes have RY000. Arguments are lowered total through `lower_arg`, which substitutes `Expr::Unknown`. |
| `lower_binary`: operator/text, lhs/rhs and child lowering | explicit | Required grammar fields; malformed omissions have RY000. Valid children are preserved, including R hex numerics. |
| `lower_unary`: operator/text, operand-or-rhs, child lowering | explicit | Required grammar fields; malformed omissions have RY000. |
| `lower_extract`: lhs lowering and rhs | explicit | Required grammar fields; malformed omissions have RY000. |
| `try_unwrap_raw_string`: opening quote, opening bracket search, close search, and the one `from_utf8(...).ok()?` | safe | This helper returns “recognized raw-string spelling”, not an AST node. `unquote_r_string` falls back to ordinary-string processing when it returns `None`, so the enclosing string expression and statement remain represented. The `.ok()?` input is the fixed ASCII two-byte sequence `[close_bracket, b'"']`. |

## `None` inventory

| Site(s) | Status | Ownership argument |
| --- | --- | --- |
| `parse_with_tree(..., None)` | safe | Selects a full parse rather than incremental reuse. |
| Top-level / braced / block `if let Some(stmt)` and `lower_stmt` fallback `None` | explicit | A parse-clean expression is lowered to a concrete variant or `Expr::Unknown`. The remaining `None` cases require a missing grammar field and are accompanied by a collected parse error. R6 checks corpus-derived statements in flat and nested multi-statement hosts. |
| `try_lower_assign` non-assignment and invalid `:=` target `return None` | safe | Control-flow sentinel only. The caller immediately runs ordinary binary lowering, so the expression is not discarded. Invalid `:=` is represented by that fallback. |
| `Stmt::FunctionDef { name: None }` | safe | Anonymous function syntax has no name in the compact AST; this is data, not propagation. |
| `Expr::If { else_: None }` | safe | R permits `if` without `else`; the value semantics are intentionally represented by absence. A malformed present alternative has RY000. |
| `Param::default`, `Arg::name`, and positional/unknown argument `name: None` | safe | These are semantic absence. Argument values use `Expr::Unknown` rather than disappearing. A malformed default has RY000. |
| Namespace lhs/rhs/text `None` match arms | fixed/explicit | They return `Expr::Unknown`, never `None`, so an enclosing expression survives. Missing fields remain additionally diagnosed by RY000. |
| `try_unwrap_raw_string` `return None` arms | safe | Recognition failure falls back to ordinary string unquoting; it cannot erase the `Expr::String`. |
| `process_r_escapes` `(None, consumed)` and `match None` arms | safe | This is an internal “no replacement character” marker. The helper either preserves the original escape or intentionally removes a physical line continuation; the string AST node remains. |
| `namespace_op` terminal `None` | safe | The caller defaults to `"::"`; the namespace expression remains represented. |
| `collect_comments` failed UTF-8 read | safe | It can omit only comment metadata, never an expression or statement. The nodes and source share a valid UTF-8 buffer, so the failure is defensive. |

## Other `Option` propagation and fallback inventory

The audit also followed propagation that does not spell a literal `None` at the
use site:

| Site(s) | Status | Ownership argument |
| --- | --- | --- |
| Root/braced/block `if let Some(stmt)` filters | explicit | These are the sinks for lowering absence. For parse-clean nodes, the lowerers are concrete or use `Expr::Unknown`; malformed recovered nodes have RY000. R6 covers both flat and nested sinks. |
| `try_lower_assign` `if let Some` | safe | A miss means “ordinary binary”, and the caller immediately invokes `lower_binary`. |
| `lower_stmt` fallback `if let Some(expr)` | explicit | The default `lower_expr` arm returns `Expr::Unknown`. A miss can come only from a recognized form with a missing required child; RY000 owns it. |
| `lower_if` alternative `.map` | safe | Directly represents the grammar's optional `else`; no present alternative is filtered. |
| `lower_if_expr` alternative `.and_then(...).map(...)` | safe / explicit | Grammar absence is semantic `else_: None`. A present alternative that cannot lower is recovered malformed syntax and has RY000. |
| `lower_while` / `lower_repeat` `unwrap_or` fallbacks | fixed-safe | Missing body/condition nodes become a block/`Expr::Unknown` while RY000 is retained; the loop statement cannot disappear. |
| `lower_params` name `unwrap_or_else` and default `.and_then` | safe / explicit | A missing name gets `"?"`; an absent default is semantic; a malformed present default has RY000. The parameter itself remains. |
| Parenthesized-expression `if let Some(first child)` | explicit | Empty parentheses are not parse-clean R and have RY000. |
| Double-brace detection `.is_some_and` | safe | Failure only chooses the ordinary `Expr::Block` representation; it never removes the expression. |
| `lower_arguments` optional node and argument-kind filter | safe | Calls may have no arguments. Non-`argument` children are punctuation/comments; every argument is passed to total `lower_arg`. |
| `lower_arg` `if let Some` chains and `unwrap_or(Expr::Unknown)` | fixed-safe | Missing values become `Expr::Unknown`; `name: None` preserves positional semantics. No argument disappears. |
| `text` `.ok().map(String::from)` | safe | Converts tree-sitter's range result; all lowering callers are classified above. Nodes and the source share the same valid UTF-8 buffer. |
| Raw-string prefix `.or_else` / `if let Some` | safe | Selects raw-string decoding. Failure falls back to ordinary decoding and retains `Expr::String`. |
| Escape Unicode `.ok().and_then(char::from_u32)` | safe | Invalid scalar conversion chooses the preserving `(None, 2)` escape path; the string expression remains. |
| Comment `if let Ok(full)` and `strip_prefix(...).unwrap_or(...)` | safe | Affect comment metadata only, never AST expression/statement representation. |
| Namespace operator `.unwrap_or("::")` | safe | Supplies a concrete operator spelling, so the namespace expression remains. |

## Sites fixed or made total

1. **Integer literal conversion (historical, `89eddd2`)** — an R integer such
   as `0x1p2L` that Rust's `i64` parser rejects now falls back to `f64` and then
   `Expr::Unknown`, rather than returning `None` through an enclosing binary or
   assignment.
2. **Float literal conversion (historical, `619e61e`)** — an R hex float such
   as `0x1.8p2` now becomes `Expr::Unknown` if Rust's decimal `f64` parser
   rejects it, rather than using `.ok()?` and erasing its statement.
3. **Nested brace lowering ** — `lower_braced_as_stmt` used to overwrite
   `last` for each child. It both intentionally discarded earlier valid
   statements and could replace a preserved child with `None`. It now returns a
   total `Stmt::Expr(Expr::Block)` containing every lowered child.
4. Existing total fallbacks remain required: unknown expressions,
   unrepresentable numerics, malformed namespace components, and argument
   values use `Expr::Unknown` instead of absence.

## Durable gates and corpus selection

`crates/ry-checker/tests/invariants.rs` owns the executable evidence:

- **R1** checks every diagnostic from every top-level checker fixture (229 at
  adoption; the gate accepts additions but refuses shrinkage below 229) for
  ordered, in-bounds UTF-8-boundary spans.
- The ecosystem side uses the vendored `glue` and `purrr` R sources. Membership
  is deterministic: sorted `.R` paths whose FNV-1a hash modulo 5 is zero. The
  gate requires at least 10 files so an absent or accidentally emptied sample
  cannot pass.
- **R6** extracts every individually parse-clean, one-statement slice from all
  checker fixtures and the same ecosystem sample, deduplicates the slices, and
  inserts each at every `k` in flat and nested multi-statement hosts. The
  statement must remain in the production AST or overlap an explicit parser
  diagnostic. Fixed adversarial candidates `0x1p2L` and `0x1.8p2` ensure that
  restoring either historical numeric `None` regression fails the gate.

The property consumes only `RParser` output and checker diagnostics. It does not
implement a second R parser.
