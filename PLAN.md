# ry rewrite plan

This plan rebuilds the core of `ry` (an R type checker modeled on astral's `ty`)
around the findings of a full-codebase review (2026-07). It is written to be
executed by an agent with no prior context. Work through the phases in order;
each phase has explicit acceptance criteria. Do not start a later phase while
an earlier phase's criteria fail.

## Context: what is wrong today (verified findings)

The workspace has 5 crates: `ry-core` (AST, types, tree-sitter parser adapter),
`ry-checker` (inference + diagnostics, 6.8k lines), `ry-typeshed` (JSON base-R
signatures), `ry-cli`, `ry-lsp` (5.2k lines, one file). All 354 tests pass, but:

1. **Named function bodies are never checked.** `f <- function() { "hello" + 1 }`
   produces zero diagnostics. Pass 3 (`check_stmt` on `Stmt::Assign`) infers the
   RHS via `infer` -> `Expr::Function` arm (`crates/ry-checker/src/lib.rs:1935`)
   -> `function_value_from_literal`, which is the pure pass-2 path that emits no
   diagnostics. Only bare statement-position `function()` literals
   (`lib.rs:1701`) and HOF callbacks (`walk_callback_for_diagnostics`) get
   diagnostic walks. Almost all real R code lives in named function bodies.
2. **Parsing is O(n^2).** `char_col` (`crates/ry-core/src/parser.rs:601`) rescans
   the file from byte 0 for every node's span. Measured: 5k lines 2.5s,
   10k lines 11.6s, 20k lines 47s. tree-sitter already provides the column via
   `node.start_position().column`; the code uses `.row` and discards `.column`.
3. **Syntax errors are silently swallowed.** tree-sitter always returns a tree;
   `root.has_error()` is never consulted. Broken files check "clean" (plus
   garbage diagnostics from error-node fragments).
4. **Two parallel inference engines.** `infer_pure_at_depth` (pass 2, no
   diagnostics) and `infer` (pass 3, diagnostics) duplicate every expression
   form and are kept in sync by hand. Finding 1 is a direct symptom.
5. **`RType: Copy` is propped up by leaked global intern tables.**
   `intern_class_name` / `intern_column_schema` / `intern_function_signature`
   (`crates/ry-core/src/types.rs:286,352,408`) `Box::leak` into
   `OnceLock<Mutex<Vec<&'static _>>>` with O(n) linear scans. Unbounded memory
   growth in LSP sessions; global state shared across checks.
6. **Parser correctness bugs:**
   - `<<-` unrecognized: `try_lower_assign` and `lower_binary` match the string
     `"<<"` (`parser.rs:108,439`), but tree-sitter-r emits `<<-`. Super-assign
     statements lower to `Expr::Unknown` and are dropped.
   - `**` mapped to `Mul` (`parser.rs:413`); in R `**` is `^` (power).
   - Integer literals that fail `i64` parse (`1e5L`, `0x10L`) return `None`,
     and `?`-propagation in `lower_binary`/`try_lower_assign` silently deletes
     the whole enclosing statement.
   - `lower_braced_as_stmt` (`parser.rs:207`) keeps only the last statement of
     a top-level `{ ... }` block.
   - String lowering strips quotes but not escape sequences; raw strings
     `r"(...)"` unhandled.
7. **Type-model semantic bugs:**
   - `arith_result` (`types.rs:54`): `(Null, x) => Some(x)` precedes the
     Character rejection, so `NULL + "a"` types as character; R errors.
   - `join` (`types.rs:572`) collapses branches via the coercion ladder:
     `if (p) 1L else "a"` types as `character`. R never coerces at `if`; the
     honest answer is a union.
   - `apply_narrowing` (`lib.rs:4245`) misuses `coerce_rank` as a subtype
     lattice: `is.numeric(x)` rewrites a known-integer to Double; `is.list(x)`
     on a character "narrows" it to List.
   - The NA flag carries no signal (`arith` sets it for every double; `infer_c`
     for every character/double).
   - `S3_GENERICS` contains `t`, `c`, `format`, ... so `t.test <- function(...)`
     is misregistered as S3 method `t` for class `"test"`
     (`split_s3_method_name`, `lib.rs:88`).
8. **Suppression parsing is textual:** `parse_suppressions` /
   `has_file_suppression` (`lib.rs:321,438`) find `#` inside string literals,
   and `ry: ignore-file` matches as a substring of any comment prose.
9. **Waste:** typeshed JSON (61KB) reparsed on every `Checker::new`;
   `Project::check` clones `FnTable` + `ReturnSlots` per file
   (`crates/ry-checker/src/project.rs:120-125`); `refine_fn_return` clones
   function bodies per fixpoint iteration; LSP rebuilds and rechecks the whole
   project on every keystroke (`crates/ry-lsp/src/lib.rs:977`).
10. **Tests measure the implementation, not R.** Every `err_*` corpus fixture
    is top-level code; the "real_world" tests print tables and assert nothing.
11. **Small stuff:** MSRV claims 1.76 but uses `Option::is_none_or` (1.82+);
    `--color` parsed and ignored; `--output-format full` behaves as `concise`;
    dead enum variants (`BinOpKind::NotIn`, `PipeBind`, `ReturnTypeSlot`);
    `Length::binary` has two identical `Known/Known` branches; no README;
    `repository = "https://example.invalid/ry"`.

## What to preserve (do not regress)

- The `RType` dimensions: mode, length, S3 class vector, column schema. The
  column-schema -> NSE augmented-scope mechanism (`subset(df, cyl == 4)`
  resolving `cyl` against the schema) and the HOF callback-return modeling are
  the project's best ideas. Keep the semantics; change the representation.
- Typeshed as data (JSON with slot-based return specs).
- The product surface: rule codes RY001-RY070 and their meanings, `# ry:
  ignore` / `# noqa` suppression, `--error/--warn/--ignore` filters, `ry.toml`
  discovery and CLI-override precedence, output formats, corpus harness format
  (`# expect:` / `# no-diag` first-line markers).
- Existing corpus fixtures must keep passing unless a fixture itself encodes a
  bug (if so, fix the fixture and note it in the commit message).

Build/test commands: `cargo build --release`, `cargo test --workspace`.
Rules from the repo owner: never run `git` commands (the user commits), no
emojis anywhere, do not add dependencies by editing Cargo.toml by hand where an
installer exists (for Rust, adding to Cargo.toml is fine — use `cargo add`).

---

## Phase 0 — Pin behavior with tests that can fail (do this first)

Goal: make the rewrite verifiable before touching any implementation.

1. **Function-body fixtures.** Add corpus fixtures under
   `crates/ry-checker/testdata/`:
   - `err_fnbody_arith.R`: `# expect: RY040` with `"a" + 1` inside
     `f <- function() { ... }`.
   - `err_fnbody_unbound.R`: `# expect: RY010` with an undefined variable
     inside a named function body.
   - `err_fnbody_nested_if.R`: `# expect: RY040` inside an `if` inside a named
     function.
   - `ok_fnbody_sequential.R`: `# no-diag` — sequential bindings inside a
     function's `if` branch (`tmp <- 1; out <- tmp + 1`) must not fire RY010.
   These will FAIL against the current code. That is the point: they define
   done for Phase 2.
2. **Parse-error fixture.** Extend the corpus harness (`tests/corpus.rs`) with
   a new marker `# expect-parse-error`, and a fixture containing broken syntax.
   Wire it to whatever parse-error reporting Phase 1 introduces.
3. **Parser round-trip tests** in `ry-core` for: `x <<- 1` (must lower to a
   super-assignment, not Unknown), `2 ** 3` (must be Pow), `1e5L` and `0x10L`
   (statement must not vanish; a fallback typed literal or Unknown *expression*
   is fine, a dropped *statement* is not), top-level `{ a <- 1; b <- 2 }`
   (both statements preserved).
4. **Performance regression test.** Add `crates/ry-checker/tests/perf.rs`
   (marked `#[ignore]` so CI opt-in): generate a 20k-line file in a tempdir,
   parse + check, assert wall time under 2 seconds. Include the generation
   snippet from this plan's appendix.
5. **Oracle harness (skeleton now, corpus later).** New test binary
   `crates/ry-checker/tests/oracle.rs`, `#[ignore]` by default, that for each
   fixture in `testdata/oracle/`: runs `Rscript --vanilla <file>` if `Rscript`
   is on PATH (skip cleanly otherwise), records whether R errors, runs the
   checker, and asserts: R-errors => at least one ry error-severity diagnostic
   (for fixtures tagged `# oracle: must-flag`), R-succeeds => no error-severity
   diagnostics (for fixtures tagged `# oracle: must-pass`). Seed it with ~10
   fixtures covering `<<-`, `**`, `NULL + "a"`, arithmetic on character,
   `$` on atomic, calling a non-function.

Acceptance: `cargo test --workspace` runs; the new function-body and parser
tests fail (expected-fail list documented in the PR/commit description);
everything previously green stays green.

## Phase 1 — Showstopper fixes inside the current architecture

These are small, surgical, and de-risk the rewrite. Land them before the big
refactor so bisection stays possible.

1. **Fix quadratic columns.** In `RParser::span` (`parser.rs:61-67`), use
   `n.start_position().column` (byte column) directly. If char columns are
   required for diagnostics, convert byte->char within the single line only
   (slice the line, count chars), never the whole file. Delete `char_col`.
   Verify with the Phase 0 perf test: 20k lines must check in well under 2s.
2. **Surface parse errors.** After parsing, if `tree.root_node().has_error()`,
   walk the tree for `ERROR` / `MISSING` nodes and emit a parse diagnostic per
   node (new rule `RY000` / name `syntax-error`, severity Error, registered in
   `rules.rs` keeping codes lexicographic). Plumb through `Checker`/`Project`
   /CLI/LSP so a broken file exits nonzero and shows the error location.
   Decision: still run the checker on the recovered tree (diagnostics beyond
   the error may be noise, ty checks anyway; match that) — but always emit the
   RY000s.
3. **Check named function bodies.** Minimal fix within the current dual-engine
   design (the real fix is Phase 2, but do not leave the hole open):
   in `check_stmt`'s `Stmt::Assign` arm, when `value` is `Expr::Function`,
   additionally walk the body with `check_stmt` in a child scope seeded with
   params (mirror the existing `Stmt::FunctionDef` arm at `lib.rs:1701-1727`).
   While here, fix the per-statement scope-clone bug in the `Stmt::If` arm
   (`lib.rs:1664-1671`): clone `then_scope` ONCE before the loop, not once per
   statement, so sequential bindings inside a branch resolve
   (`ok_fnbody_sequential.R` covers this).
4. **Parser bug batch:** `"<<"` -> `"<<-"` in both match sites; map `**` to
   `Pow`; integer-literal fallback (on `i64` parse failure lower to
   `Expr::Double` if it parses as f64, else `Expr::Unknown(span)` — never
   propagate `None` upward from a literal); make `lower_braced_as_stmt`
   preserve all statements (either introduce `Stmt::Block(Vec<Stmt>)` or
   return multiple statements via a smallvec/Vec — `lower_stmt` callers must
   splice).
5. **Typeshed loaded once.** Wrap `load_base()` in a `OnceLock<Typeshed>` (or
   `LazyLock`) and have `Checker::new`/`with_tables` take `&'static Typeshed`
   or a cheap `Arc<Typeshed>`.

Acceptance: Phase 0's function-body, parser, and perf tests pass. Full suite
green. Manual probe: the three showstopper snippets from the review behave
(function-body errors flagged; 20k lines < 2s; broken file reports RY000 and
exits nonzero).

## Phase 2 — Single inference engine

Goal: delete the pure/impure duplication; one engine, one truth.

1. Introduce a sink abstraction in `ry-checker`:
   ```rust
   pub(crate) trait DiagSink { fn emit(&mut self, d: Diagnostic); }
   pub(crate) struct Collect<'a>(&'a mut Vec<Diagnostic>);
   pub(crate) struct Discard;
   ```
   (Or an enum; trait-object vs generic is implementor's choice — measure, but
   a generic parameter monomorphizes fine here.)
2. Rewrite the expression/statement walkers as ONE set of functions
   parameterized by the sink: `infer(&self_ctx, expr, &mut scope, &mut sink)`.
   Pass 2 (fixpoint refinement) calls with `Discard`; pass 3 with `Collect`.
   Fold in, and then DELETE: `infer_pure_at_depth`, `infer_binop_pure`,
   `infer_c_pure`, `apply_sig_pure`, and every "mirrors pass 3 minus
   diagnostics" comment. `collect_returns_and_simulate_at_depth` becomes the
   single body-walking routine used by both return inference and diagnostics
   (diagnostics emission is just the sink choice).
3. Function bodies get exactly one walking policy: when pass 3 encounters a
   function literal (named or anonymous, assigned or bare), it walks the body
   once with `Collect` in a child scope (params seeded from defaults/UNKNOWN,
   enclosing bindings visible). Return-type inference reuses the same walk's
   result rather than re-walking. Remove the Phase 1 stopgap double-walk.
   Guard against double-emission: a body must be walked with `Collect` exactly
   once per check (fixpoint iterations use `Discard`).
4. Scope semantics: pick ONE model and document it in the module header.
   Recommended v1 model: statements in a block share a scope sequentially;
   `if` branches each get a child scope cloned from the parent; bindings from
   branches do NOT merge back (conservative; matches current pass-3 intent,
   minus the per-statement clone bug). Loops walk once with a child scope.
5. Keep the 3-pass shape (collect -> fixpoint -> emit) but stop cloning
   function bodies per refinement: store bodies once (e.g. `Rc<[Stmt]>` in
   `UserFn`) and iterate by reference.

Acceptance: all corpus fixtures pass, including Phase 0 additions; line count
of `ry-checker/src/lib.rs` drops substantially (expect roughly -1.5k lines);
grep finds no `_pure` inference functions; running the checker twice on the
same input yields identical diagnostics (add a determinism test).

## Phase 3 — Type representation: kill the leaked globals, add unions

1. **Session-owned interner.** New `TypeInterner` struct owned by the check
   session (`Checker` / `Project`), not a global:
   - `RType` stops being `Copy`-via-leak. Two acceptable designs; pick one:
     a. ID-based (recommended, matches ty/ruff): `Ty(u32)` handles, all
        structural data lives in the interner; `RType` becomes a small POD of
        ids + enums and stays `Copy`.
     b. `Arc`-based: `class: Option<Arc<ClassVec>>`, `columns:
        Option<Arc<ColumnSchema>>`, `fn_sig: Option<Arc<FunctionSignature>>`,
        `RType: Clone` (cheap). Simpler; fine if (a) is too invasive.
   - Delete `intern_class_name`, `intern_column_schema`,
     `intern_function_signature` and their `OnceLock<Mutex<...>>` tables.
     Nothing may call `Box::leak` in the type layer.
   - Interner lookups must be hashed (HashMap keyed by content), not linear
     scans.
2. **Union type.** Add `Mode::Union` support via the interner: a bounded union
   (cap at ~4 members like ty's approach to literal unions; join collapses to
   UNKNOWN beyond the cap). `join` becomes: equal -> self; otherwise build a
   union of the two (deduplicated), never coercion-ladder promotion.
   Update consumers: `arith`/`compare` over a union distribute over members
   (error only if ALL members error; warn if SOME error is out of scope for
   now); `invalid_condition` true only if all members invalid. Fixtures:
   `x <- if (p) 1L else "a"; y <- x + 1` should NOT be an error (character
   member would error, integer member is fine -> stay quiet in v1),
   and `x <- if (p) list(1) else function() 1; x + 1` SHOULD error (all
   members invalid).
3. **Fix arith/compare tables** (`types.rs`): move the Character/List/Function
   rejections BEFORE the Null arm so `NULL + "a"` is an error; keep
   `NULL + 1` -> numeric-with-length-0 semantics (R returns `numeric(0)`;
   model as `Length::Zero`). Delete the duplicate `Known/Known` branches in
   `Length::binary` (single arm).
4. **Narrowing rewrite** (`apply_narrowing`): replace coerce_rank comparisons
   with explicit compatibility: a predicate narrows only when the existing
   mode is Opaque or a union containing the predicate's mode; `is.numeric`
   narrows to union(integer, double), never rewriting a known Integer to
   Double; incompatible known modes leave the scope untouched (optionally
   flag dead branch later — out of scope).
5. **NA flag decision:** remove `NaFlag` from `RType` entirely OR make it
   honest (literals: false; `NA` literals: true; `c()`/arith: OR of inputs
   only — no blanket true-for-double). Recommended: remove; no diagnostic
   consumes it. If removed, update Display and typeshed JSON handling
   (ignore the `na` field on load).
6. **S3 method-name splitting:** replace the generous `S3_GENERICS` prefix
   scan with a curated table and an explicit denylist of well-known
   dotted-but-not-method names (`t.test`, `all.equal`, `as.data.frame` when it
   IS the generic, `file.path`, `Sys.time`, ...). Registration should also
   require the function's first parameter to be named like the generic's
   (usually `x` or matching typeshed) — cheap heuristic, cuts most collisions.
   Delete the `[0u8; 64]` stack-buffer prefix construction; use `format!`.

Acceptance: no `Box::leak` in the workspace (`grep -rn "Box::leak" crates/` is
empty); memory does not grow across repeated checks (add a loop-100-checks
test asserting interner size stabilizes for identical input); union fixtures
pass; all prior corpus fixtures still pass (some may need updating where they
encoded coercion-ladder joins — update them deliberately and say so).

## Phase 4 — Lossless parsing and suppression correctness

1. **Statement-drop audit.** Grep `ry-core/src/parser.rs` for every `?` /
   `None` return in `lower_stmt`/`lower_expr` paths and ensure the invariant:
   a lowering failure inside an expression yields `Expr::Unknown(span)`; a
   statement is dropped ONLY for pure comment/whitespace nodes. Add a debug
   assertion counter test: for each corpus file, number of lowered top-level
   statements equals the number of named non-comment top-level CST children.
2. **String literals:** process escape sequences (`\"`, `\\`, `\n`, `\t`,
   `\u{...}` at minimum) and handle raw strings `r"(...)"` / `R"[...]"` by
   slicing the delimiters. Column-name matching (`df$"my col"` and
   `list("a b" = 1)`) depends on this being right.
3. **Suppression comments become lexical.** Reimplement
   `parse_suppressions` / `has_file_suppression` on tree-sitter comment
   tokens instead of `line.find('#')`: collect all `comment` nodes with their
   line numbers during parse (expose from `ry-core`), then parse directives
   from those texts only. `ignore-file` must anchor: the comment body, after
   trimming, must START with `ry: ignore-file` (no substring matching).
4. **`%<>%` semantics:** in `infer_pipe`, when the overall expression is
   `lhs %<>% rhs` and `lhs` is an identifier, rebind the identifier in scope
   to the result type. Delete `BinOpKind::NotIn` and `PipeBind` (unproduced)
   or wire `%notin%` if trivially available in the grammar — deleting is fine.

Acceptance: new parser tests pass; a fixture with `x <- "# noqa"` followed by
a real diagnostic on the same line still reports the diagnostic; corpus green.

## Phase 5 — Batch performance and plumbing hygiene

1. `Project::check` pass 3: stop cloning `FnTable`/`ReturnSlots` per file.
   Restructure `Checker` so the shared tables are borrowed (`&FnTable`,
   `&ReturnSlots`) by an emitter that owns only per-file state. If borrow
   structure fights the current `&mut self` methods, wrap shared tables in
   `Rc` and clone the `Rc`, not the tables.
2. One `RParser` per run, not per file (CLI `run_check_once` and LSP both
   construct per-file parsers today).
3. CLI honesty: implement `--color` (or remove the flag); implement `full`
   output format with a source snippet + caret line (the `srcs` map is already
   plumbed into `render`); `print_summary` should not print the human summary
   line when the format is json/gitlab/junit (machine consumers).
4. Metadata: set `rust-version = "1.82"` (or drop `is_none_or` and keep 1.76 —
   pick one, verify with `cargo +1.x check` note in commit message); fix
   `repository`; write a README.md (what works, what does not, how to run,
   rule table generated from `rules.rs`).
5. Delete dead code: `ReturnTypeSlot` in ry-typeshed, unused enum variants,
   the identical-branch code found in review. Run `cargo clippy --workspace
   -- -D warnings` and fix what it finds; add that to the acceptance bar.

Acceptance: `cargo clippy --workspace -- -D warnings` clean; perf test still
under budget; `ry check --output-format json` emits only JSON on stdout.

## Phase 6 — LSP rework (bounded scope)

Do NOT grow LSP features in this phase. Make what exists correct and cheap.

1. **Position handling:** implement UTF-16 <-> byte offset conversion for LSP
   positions (negotiate `positionEncoding` if the client offers utf-8). All
   find-identifier logic must go through AST lookup: add a
   `node_at_position(file, byte_offset) -> Option<&Expr/ident span>` query in
   `ry-core` (walk the AST spans; they exist) and delete the byte-scanning
   `find_identifier_at_position` family. R identifiers are not ASCII-only.
2. **Debounce + cache:** keep parsed `SourceFile`s in `State` keyed by doc
   version; on `did_change`, re-parse ONLY the changed doc; re-run the project
   check debounced (~150ms) rather than per keystroke. Full incrementality
   (salsa) is explicitly out of scope for this plan — but structure the entry
   point as a pure function `check_project(docs: &BTreeMap<Path, SourceFile>)
   -> Diagnostics` so a query system can be introduced behind it later.
3. **Split the file.** `ry-lsp/src/lib.rs` (5.2k lines) into modules:
   `backend.rs` (LanguageServer impl), `position.rs`, `symbols.rs`,
   `navigation.rs` (defs/refs/rename/highlight), `hints.rs`
   (inlay/completion/signature), `folding.rs`, `diagnostics.rs`. No behavior
   changes in the split commit.

Acceptance: existing LSP unit tests pass; new tests for UTF-16 position
mapping on a line containing non-ASCII; a rename on a non-ASCII identifier
works; typing in a 30-file project does not re-parse unchanged files (assert
via a parse-counter in tests).

## Phase 7 — Grounding: the R oracle corpus

Promote the Phase 0 skeleton into the project's main quality gate.

1. Expand `testdata/oracle/` to 40+ fixtures spanning: coercion ladder, `:`
   semantics (`-1:3`, fractional endpoints), recycling warnings, `[[` vs `[`
   vs `$`, S3 dispatch with and without `default`, NSE verbs, HOFs, closures
   and `<<-` state, `switch` fallthrough (`a=`, `b=2`), `tryCatch`.
2. CI-style script `scripts/oracle.sh`: runs the ignored oracle tests when
   `Rscript` is available (`cargo test --test oracle -- --ignored`).
3. Vendor ONE small real CRAN package's R sources (pick something base-R-only,
   e.g. `glue`'s R/ directory or similar, ~1-2k lines) under
   `testdata/vendor/<pkg>/`, license permitting (MIT-licensed package; keep
   its LICENSE file). Add a snapshot test (use `insta`; add via `cargo add
   insta --dev -p ry-checker`) asserting the exact diagnostic list. Every
   diagnostic in that snapshot must be triaged in a comment: true positive or
   known-limitation with a linked plan item. The snapshot failing = the
   false-positive alarm the current "baseline report" tests pretend to be.
   Then delete or convert the assertion-free `real_world.rs` printers.

Acceptance: oracle suite green locally with R installed; vendor snapshot
committed and triaged; `real_world.rs` no longer contains assertion-free
tests.

---

## Explicit non-goals for this plan

- Salsa/incremental computation. Deferred deliberately; Phase 6.2's pure-entry
  refactor is the seam where it lands later. Do not hand-roll a cache layer.
- New diagnostics/rules beyond RY000 (syntax-error).
- S4/R6/environments modeling, package DESCRIPTION/NAMESPACE resolution,
  cross-package typeshed generation. All future work.
- LSP feature additions (semantic tokens, formatting, etc.).

## Sequencing and commit discipline

- One phase per PR-sized change set; within phases, the numbered items are
  commit-sized. Phase 1 items 1-3 may land as separate commits on day one.
- Never mix a behavior fix with the Phase 2/3 refactors in one commit.
- Each commit message states which plan item it implements and which
  acceptance tests cover it. The user runs git themselves; prepare changes
  and report — do not commit unless asked.

## Appendix: verification probes

Perf file generator:
```
python3 - <<'EOF'
lines = [f'x{i} <- c({i}, {i+1}) * 2' for i in range(20000)]
open('big.R','w').write('\n'.join(lines))
EOF
```
Budget: `ry check big.R` under 2s release-mode.

Showstopper probes (all must produce the noted result after Phase 1):
```r
# must flag RY040 + RY010:
f <- function() { x <- "hello" + 1; y <- undefined_variable_xyz; x }
# must flag RY040 (super-assign recognized):
x <<- "a" + 1
# must not lose statements (m unbound would be a bug; n must be bound):
n <- 1e5L
m <- n + 1
# must report RY000 syntax error, nonzero exit:
f <- function( {
  broken syntax ((
# must NOT warn RY010 on tmp/out (sequential branch bindings):
g <- function(flag) { if (flag) { tmp <- 1; out <- tmp + 1 }; NULL }
```
