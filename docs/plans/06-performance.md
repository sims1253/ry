# Plan 06: Performance — LSP incrementality, hot paths, benchmarks

## Status: blocked on plan 01 (module split). Independent of 02-05.
## Repo: ry (this repo).

## Context

Three problems, in descending order of user impact:

1. **LSP rebuilds the world per keystroke.** `crates/ry-lsp/src/backend.rs`
   (~line 1068) documents that every `didChange` rebuilds a full `Project`
   from ALL open documents: reparse every document, re-run pass 1 collection,
   the pass-2 fixpoint, and pass-3 emission for every file.
2. **No benchmarks.** `crates/ry-checker/tests/perf.rs` is a 2-second
   wall-clock gate over synthetic code; it cannot catch 20% regressions.
   jarl (sibling project) advertises 0.131s over 25k lines; ry has no
   comparable number.
3. **Avoidable allocations in hot paths** (see below).

## Part A: benchmarks FIRST (so B and C are measurable)

- Add `criterion` as a dev-dependency of ry-checker with a `benches/`
  directory. Benchmarks:
  - `parse_large`: parse the concatenation of all vendored glue sources
    (`crates/ry-checker/testdata/vendor/glue/R/*.R`).
  - `check_project_glue`: full `Project::check` over the glue sources
    (parse excluded — pre-parse in setup).
  - `check_single_synthetic`: the existing perf.rs 20k-line synthetic,
    single `Checker::check`.
  - `lsp_edit_sim`: simulate the LSP loop — build a Project from glue, then
    in the bench iteration mutate one file's source (append a newline +
    statement), reparse only what the current implementation reparses, and
    re-check. After Part B this should show the incremental win.
- `cargo bench -p ry-checker` must run them. Record baseline numbers in the
  final summary (before/after for parts B/C).

## Part B: LSP incrementality (cheap wins, NOT salsa)

Read `crates/ry-lsp/src/backend.rs` carefully first; then:

1. **Per-document parse cache.** Keep the parsed `SourceFile` (and the
   tree-sitter `Tree` if the parser API exposes it) per open document,
   keyed by document version. On `didChange`, reparse ONLY the changed
   document. If ry-core's `RParser` supports tree-sitter incremental
   parsing (`Tree::edit` + reparse with old tree), use it — check
   `crates/ry-core/src/parser.rs`; if it does not, add an
   `RParser::reparse(&mut self, old: &SourceFile, new_text: &str, edits)`
   entry point only if tree-sitter's API makes it straightforward;
   otherwise full reparse of the single changed file is acceptable for this
   plan (still a big win: N files -> 1).
2. **Cache pass-1 collection per file.** `Project::check`
   (`crates/ry-checker/src/project.rs`) re-collects every file each call.
   Add an incremental entry point, e.g.
   `Project::update_file(path, SourceFile)` + `Project::check_incremental()`
   or a builder that accepts pre-collected per-file tables. Design
   constraint: keep the existing `check()` untouched for the CLI; the
   incremental path may be LSP-only. Per-file collection results
   (fn definitions, S3 registrations, loaded packages, declared globals)
   get cached keyed by document version; only the edited file re-collects.
   The pass-2 fixpoint re-runs over the merged tables (it converges in 2-3
   iterations; fine), and pass 3 re-emits for all files (rayon-parallel
   already) — optimization of pass 3 re-emission is OPTIONAL; only do the
   "skip re-emitting files whose diagnostics cannot have changed" analysis
   if it falls out simply (it usually does not — cross-file fn types can
   change any file's diagnostics).
3. **Debounce** didChange re-checks (e.g. 150ms) if the backend does not
   already; check for an existing tokio-based debounce pattern in
   backend.rs first.

## Part C: allocation hot paths

Verify each with the benchmarks (criterion comparison) — do not blind-fix:

1. `split_s3_method_name` (pre-split lib.rs ~line 211): allocates
   `format!("{}.", generic)` for each of ~21 generics per candidate name.
   Rewrite with `name.strip_prefix(generic).and_then(|r| r.strip_prefix('.'))`
   — zero allocation. (Called per top-level assignment.)
2. `Project::check` pass 3 (project.rs ~line 210): `(*loaded).clone()` — a
   full `HashSet<String>` clone per file. Change `Checker` to hold
   `Arc<HashSet<String>>` for `loaded` (or `Cow`); mirror for the other
   per-file cloned maps where they are read-only in pass 3
   (`external_bindings.get(path).cloned()` is per-file data and small — fine
   to leave).
3. `ColumnSchema.columns: Vec<(String, RType)>` deep-clones on column
   assignment (`type_with_assigned_column` clones the whole schema). Change
   the column name to `Arc<str>` if the churn is contained (check all
   construction sites in ry-core types.rs + ry-checker); ONLY do this if the
   benchmarks show schema cloning matters on the glue corpus; otherwise
   record "measured, not significant" in the summary.
4. Run a quick allocation profile if available (`cargo flamegraph` or
   `perf`) — optional, only if the tooling is already installed; do not
   install system packages.

## Rules

- Behavior/diagnostic output must be unchanged (corpus + vendor_snapshot +
  full test suite green, no snapshot updates).
- LSP protocol behavior unchanged (existing ry-lsp tests in
  `crates/ry-lsp/src/tests.rs` must pass; extend with an incremental test:
  two documents, edit one, diagnostics for both remain correct — especially
  the cross-file case where editing `utils.R`'s function signature changes
  diagnostics in `analysis.R`).
- No mutating git commands. No emojis. No new runtime dependencies except
  criterion (dev) — Arc changes use std.
- Record before/after bench numbers for every optimization in the final
  summary.

## Acceptance criteria

- `cargo bench -p ry-checker` runs 4 benchmarks.
- `lsp_edit_sim` (or equivalent measurement) shows a substantial improvement
  (expect at least 2-5x on a multi-file project; report actuals).
- Cross-file LSP correctness test passes.
- Full workspace test suite green; clippy no new warnings; fmt clean.
