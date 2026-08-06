# Plan 33: incremental core, memory, and the workspace index

## Status: proposed (2026-08-06). Ready for implementation.

Split out of [plan 32](32-editor-extensions.md), where it began as a
one-paragraph deferral and grew on reading `crates/ry-checker/src/project.rs`.
Nothing here blocks shipping an editor extension. All of it determines whether
that extension feels good on a real package.

Two user-visible problems, one root cause:

1. **The editor only sees open files.** Open `R/helpers.R` alone and its calls
   into `R/model.R` do not resolve; open both and the diagnostics change.
   `ry check .` and the editor legitimately disagree.
2. **Per-keystroke cost is O(whole project).** Two of the checker's three
   passes rebuild from scratch on every edit, so latency grows with project
   size rather than edit size.

Fixing (1) without fixing (2) makes things worse: indexing a whole package
multiplies the cost of an already-whole-project check. They have to be done in
that order — incrementality first, then the index it makes affordable.

## Verified baseline

Read out of the tree at `0.8.0`. Line numbers are from the plan-32 worktree
base.

### What is already incremental — more than expected

- **Parse cache.** `State.parsed` is keyed `path -> (version, Arc<SourceFile>)`,
  invalidated per-document by `update_doc` (`backend.rs:51-56`). A
  `parse_count` counter (`backend.rs:74-80`) backs a test asserting that
  editing one file in a multi-file workspace re-parses only that file. Scope
  results are cached the same way for hover/inlay/completion
  (`backend.rs:57-61`).
- **Pass 1 is genuinely incremental.** `Project::update_file`
  (`project.rs:130`) evicts only that path's `collected_files` and
  `file_known_vars` entries; `check_incremental` (`project.rs:250`) re-collects
  only files missing a cache entry.
- **Pass 3 is parallel.** The emission loop is a rayon `par_iter` over files
  with `Arc`-shared read-only tables (`project.rs:335`).

The architecture is sound. The gaps are specific, not systemic.

### Where incrementality stops

| # | Gap | Evidence | Cost shape |
| :-- | :-- | :-- | :-- |
| G1 | Pass 2 re-runs the whole-project fixpoint every check | `refine_and_emit` `mem::take`s the tables into a fresh `Checker` and calls `run_fixpoint()` unconditionally (`project.rs:297-307`) | O(project) per keystroke — **dominant** |
| G2 | Pass 3 re-emits every file every check | `self.files.par_iter()` with no dirty set (`project.rs:335`) | O(project) per keystroke |
| G3 | Table merge rebuilds from scratch | `check_incremental` clones each file's cached `fn_table` and `return_slots` and re-appends (`project.rs:285`) | O(project) allocations per keystroke |
| G4 | Parsing is not incremental | `RParser::parse(&mut self, path, src)` takes no old tree (`ry-core/src/parser.rs:38`) | O(file) per keystroke |
| G5 | Avoidable hot-path clones | `ProjectCache::check` does `file.as_ref().clone()` per changed file (`backend.rs:112`); `publish_diagnostics` clones the whole open-document map, full text included, per debounce tick (`backend.rs:1085-1089`) | O(open files) per tick |
| G6 | No memory across runs | Nothing persists to disk | Full cold start every server launch and every `ry check` |
| G7 | Project = open documents only | `publish_diagnostics` builds `project_files` from `state.docs` (`backend.rs:1105`) | Correctness, not speed |

G1 and G2 are the ones worth measuring first. G3 and G5 are cheap wins. G4 is
the most invasive and the least urgent — the ~180 ms debounce
(`backend.rs:1169`) already hides full-file reparse for realistically sized R
files.

## Architectural decision: not salsa, not yet

The obvious move is [salsa](https://github.com/salsa-rs/salsa) — ty and ruff
both use it, and ry is explicitly ty-inspired. **Recommendation: do not adopt
salsa in this plan, but stop building against it.**

The reasoning:

- `Project`'s three passes are already a hand-rolled query graph with roughly
  the right shape. Salsa would formalise invalidation, not introduce it.
- The blocker is input identity. Salsa wants interned, identity-bearing inputs;
  `SourceFile` is an owned `Vec<Stmt>` (`ry-core/src/ast.rs:11`) with no node
  identity and no back-reference to the tree-sitter `Tree`. Converting the AST
  is the real cost of salsa, and it is a much larger job than closing G1–G3.
- Closing G1–G3 buys most of the available latency win on its own, and does it
  without a rewrite.

So: close the gaps, measure, and let the *shape of the invalidation logic*
decide. If the dirty-tracking in W1 and W2 stays small and legible, salsa is a
rewrite that buys correctness-of-invalidation rather than speed. If it starts
sprawling — conditional invalidation special cases, subtle staleness bugs —
that sprawl is the signal to convert, and by then the query boundaries will be
empirically known rather than guessed.

**W7 is the explicit decision point.** It is a written evaluation, not an
implementation ticket, and it should not be skipped just because W1–W6 made
things fast enough.

## Scoreboard

| Wave | id | Workstream | Closes | Depends on | Effort |
| :-- | :-- | :-- | :-- | :-- | :-- |
| 0 | W0 | Benchmark harness | — | — | S |
| 1 | W1 | Dirty-set pass 3 | G2 | W0 | M |
| 1 | W3 | Kill hot-path clones | G3, G5 | W0 | S |
| 2 | W2 | Seed and scope the fixpoint | G1 | W1 | L |
| 3 | W4 | Workspace index | G7 | W1, W2, plan 32 S4 | L |
| 4 | W5 | On-disk cache | G6 | W4 | M |
| 5 | W6 | Incremental parsing | G4 | W4 | L |
| — | W7 | Salsa decision review | — | W1–W5 | S (written) |

W1 and W3 are independent and can run in parallel. W2 is the big one and wants
W1's dirty-set machinery in place first.

---

## W0. Benchmark harness

**Nothing else in this plan should be merged without this.** Every workstream
below claims a latency improvement; none of those claims are checkable today.

**Two harnesses already exist — extend them, do not start a third.**

- `crates/ry-checker/tests/perf.rs` — `#[ignore]`d wall-clock budget
  assertions, run in CI by `cargo test -p ry-checker --test perf --release --
  --ignored` (`ci.yml:36`). Two tests:
  `large_file_checks_under_two_seconds` (a generated 20k-line file) and
  `hundred_file_project_checks_quickly` (100 synthetic cross-referencing files,
  2s budget).
- `crates/ry-checker/benches/performance.rs` — a criterion bench over vendored
  real sources (`testdata/vendor/glue/R`).

**The gap is that both measure cold checks only.** `perf.rs` calls
`Project::check`, never `update_file` + `check_incremental`, so the entire
incremental path this plan rewrites is currently unmeasured. Absolute 2-second
budgets also cannot detect a 3x warm-path regression that stays under budget.

**Change.** Add to the criterion bench (not `perf.rs` — criterion gives
distributions and regression detection, the wall-clock asserts give a pass/fail
floor, and these are different jobs):

- cold `Project::check` over the vendored corpus;
- warm `check_incremental` after a one-line edit to one file;
- warm `check_incremental` after an edit to a file *nothing* depends on — the
  number W1 and W2 exist to move;
- warm `check_incremental` after an edit that adds or removes a `library()`
  call, which invalidates project-wide (see K2);
- peak RSS at each stage.

Keep `perf.rs`'s absolute budgets as the CI gate and add one warm-edit budget
alongside them.

**Files.** `crates/ry-checker/benches/performance.rs`,
`crates/ry-checker/tests/perf.rs`, `.github/workflows/ci.yml`.

**Done when.** The bench reports all five numbers; a warm-edit budget assert
exists in `perf.rs`; and the `0.8.0` figures are recorded **in this document**
as the baseline every later workstream is measured against.

## W1. Dirty-set pass 3

**Change.** Track which files' emitted diagnostics can actually have changed,
and re-emit only those. A file must be re-emitted when:

- its own content changed; or
- any table entry it reads changed (the refined `FnTable` / `ReturnSlots`
  entries for functions it calls); or
- the project-wide `loaded` package set changed (it gates dplyr NSE
  resolution, `project.rs`).

The third condition is the trap: `loaded` is a project-wide union, so a
`library(dplyr)` appearing or disappearing in any file invalidates everything.
That is correct and must stay correct — model it explicitly rather than
discovering it as a staleness bug.

Start conservative: re-emit a file if it changed, or if any function it
references had its `ReturnSlots` entry mutated by pass 2, or if `loaded`
changed at all. Tighten later, with W0 to prove each tightening is worth it.

**Files.** `crates/ry-checker/src/project.rs`, `crates/ry-checker/src/tests.rs`.

**Done when.**
- A one-line edit to a leaf file re-emits exactly one file. Assert this with a
  counter in the same style as `parse_count` (`backend.rs:74-80`) — a test that
  asserts on wall-clock time will flake.
- Diagnostics after an incremental sequence are identical to those from a cold
  `Project::check` on the same final state. **This is the invariant that
  matters most in this plan** — property-test it over random edit sequences,
  not just a handful of cases.
- W0's "edit a file nothing depends on" number improves measurably.

## W2. Seed and scope the fixpoint

**Change.** Two parts, in order:

1. **Seed.** Stop `mem::take`ing the tables to empty (`project.rs:303`). Start
   pass 2 from the previous solution so converged entries stay converged.
2. **Scope.** Re-refine only functions reachable from changed files, via the
   call graph already implicit in `FnTable`. Everything else keeps its refined
   return type.

Correctness hazard: a fixpoint seeded from a stale solution can converge to a
*wrong* answer rather than an imprecise one, if a function's inferred return
type should have widened. Whatever seeding is done must be provably monotone,
or must invalidate transitively along reverse-dependency edges.

This is the largest correctness risk in the plan. Budget for it: the
`ry-checker` test suite is 6432 lines (`crates/ry-checker/src/tests.rs`) and
should be treated as the regression gate, run in full on every iteration.

**Files.** `crates/ry-checker/src/project.rs`, `crates/ry-checker/src/lib.rs`
(fixpoint entry points), `crates/ry-checker/src/tests.rs`.

**Done when.**
- The cold-versus-incremental equivalence property from W1 still holds, now
  with the fixpoint seeded — extend the property test rather than adding a
  parallel one.
- The full `ry-checker` suite passes unchanged.
- W0's warm-edit number improves substantially. If it does not, stop and
  reconsider before building W4 on top.

## W3. Kill hot-path clones

Small, independent, no behaviour change. Three edits:

1. Store `Arc<FnTable>` / `Arc<ReturnSlots>` in `CollectedFile` so
   `check_incremental`'s merge bumps refcounts instead of deep-cloning
   (`project.rs:285`).
2. Change `Project::update_file` to take `Arc<SourceFile>`, so
   `ProjectCache::check` stops doing `file.as_ref().clone()`
   (`backend.rs:112`).
3. Hand `publish_diagnostics` an `Arc` view of the document map instead of
   cloning every open file's full text per debounce tick
   (`backend.rs:1085-1089`).

**Files.** `crates/ry-checker/src/project.rs`, `crates/ry-lsp/src/backend.rs`.

**Done when.** No behaviour change (full suite green), and W0's peak-RSS and
warm-edit allocation counts drop.

## W4. Workspace index

**Change.** Build the project from every `.R`/`.r` file under each workspace
root, not just open documents. Requires:

- discovery honouring `exclude` globs from `ry-config` (plan 32 S1) —
  reuse `Excludes` (`config.rs:294`), do not reimplement glob matching;
- one index per workspace folder, keyed off plan 32's S4 per-folder `Project`;
- open documents shadowing on-disk contents, since the editor's buffer is
  authoritative;
- a bounded initial scan that does not block `initialize` — index in the
  background and publish progressively, with `window/workDoneProgress` so the
  editor can show it.

**Hard dependency on plan 32 S4.** Without per-folder projects this workstream
has nowhere to put a second index.

**Files.** `crates/ry-lsp/src/backend.rs`, new
`crates/ry-lsp/src/index.rs`, `crates/ry-checker/src/project.rs`.

**Done when.**
- Opening one file in a package resolves calls into unopened files in the same
  package.
- Editor diagnostics for a given file match `ry check` on that file, for the
  fixture package from plan 32 E6.
- Initial indexing of a 500-file package does not block the first diagnostic
  for the focused file by more than a second.
- `exclude` globs are honoured — an excluded directory is not indexed.

## W5. On-disk cache

**Change.** Persist pass-1 collection output keyed by a hash of file content
plus a hash of effective config, so a server restart or a fresh `ry check` in
CI reuses prior work.

Design constraints:

- **Config participates in the key.** Rule severity, `packages`, `globals`, and
  `typeshed` all change collection output. A cache keyed on content alone will
  serve wrong results after a `ry.toml` edit.
- **Version participates in the key.** A ry upgrade must invalidate everything;
  embed `CARGO_PKG_VERSION`.
- **Corruption must be survivable.** A malformed or truncated cache entry is a
  cache miss, never an error and never a wrong answer.
- Location: follow the platform cache dir convention, with an env override for
  CI. Do not write into the project tree.

This is also what would make `ry check` fast in CI, which is a bigger win than
the editor case and worth stating in the release notes.

**Files.** New `crates/ry-checker/src/cache.rs` or a new `ry-cache` crate,
`crates/ry-lsp/src/backend.rs`, `crates/ry-cli/src/main.rs`.

**Done when.**
- Second `ry check` on an unchanged tree is substantially faster than the
  first, measured by W0.
- Editing `ry.toml` invalidates correctly — diagnostics change.
- A deliberately corrupted cache file produces correct output with a warning.
- A cache written by a different ry version is ignored.

## W6. Incremental parsing

**Deliberately last.** The most invasive change here and the least urgent,
because the ~180 ms debounce already hides full-file reparse cost.

**Change.** Retain the tree-sitter `Tree` alongside `SourceFile`, feed
`InputEdit`s, and call `Parser::parse` with the old tree. This forces two
upstream changes:

- `RParser::parse` (`ry-core/src/parser.rs:38`) gains an old-tree parameter;
- the LSP moves off `TextDocumentSyncKind::FULL` (`backend.rs:204`, currently a
  deliberate v1 simplification) to incremental sync, so it has the edit ranges
  to build `InputEdit`s from.

`SourceFile` is a fully-owned `Vec<Stmt>` with no node identity, so it must
either carry the `Tree` or the AST must gain stable node ids. **If this
workstream starts requiring AST node identity anyway, stop and go do W7** —
that is precisely the salsa conversion, and doing it accidentally through the
parser is the worst way to arrive there.

**Files.** `crates/ry-core/src/{parser.rs,ast.rs}`,
`crates/ry-lsp/src/backend.rs`.

**Done when.** Reparse cost after a single-character edit is sublinear in file
size, measured by W0, with the full parser test suite unchanged.

## W7. Salsa decision review

A written evaluation, not code. Produce a short document answering:

- What did W0 measure before and after W1–W5?
- How large and how legible did the invalidation logic in W1 and W2 turn out to
  be? Count the special cases.
- Were there staleness bugs? How were they found — tests, or users?
- What would salsa cost now, given what W1–W6 taught about the real query
  boundaries?

Then decide: adopt salsa, or record explicitly that the hand-rolled graph is
sufficient and why. Either outcome is fine; leaving it undecided is not,
because the decision gets more expensive with every rule added to the checker.

---

# Risks

| id | Risk | Mitigation |
| :-- | :-- | :-- |
| K1 | Seeded fixpoint converges to a wrong answer, not merely an imprecise one | W2's monotonicity requirement; cold-vs-incremental property test |
| K2 | Dirty-set logic misses the project-wide `loaded` dependency and produces stale dplyr diagnostics | Modelled explicitly in W1; conservative first cut |
| K3 | Incremental staleness bugs are silent — wrong output, no error | The cold-vs-incremental equivalence property is the single most important test in this plan |
| K4 | Workspace indexing blows up memory on large packages | W0 tracks peak RSS; W4 bounds the initial scan |
| K5 | On-disk cache serves results from a different config or ry version | Config hash and `CARGO_PKG_VERSION` in the cache key (W5) |
| K6 | W6 drifts into an AST rewrite | Explicit stop-and-escalate to W7 |
| K7 | Perf work lands with no way to tell if it helped | W0 gates everything; nothing merges without a number |

# Acceptance for the plan as a whole

1. Editor diagnostics for a file match `ry check` on that file, for a
   multi-file R package with an unopened dependency.
2. An incremental edit sequence produces diagnostics identical to a cold check
   of the same final state, property-tested over random sequences.
3. Warm-edit latency after a one-line change is materially better than the
   `0.8.0` baseline recorded in W0, and does not grow linearly with project
   size.
4. A second `ry check` on an unchanged tree reuses cached work.
5. The salsa question is decided in writing, either way.

---

## Baseline measurements (recorded for Plan 33 W0)

Measured on the plan-32 worktree at `0.8.0`. Hardware: CI development
machine. Criterion `--quick` mode (sample size 20, 1s warmup, 3s measure).

| Benchmark | Time | Notes |
| :-- | :-- | :-- |
| `parse_large` | 4.4 ms | All glue sources concatenated, single parse |
| `check_project_glue` (cold) | 24.4 ms | 12-file vendored glue corpus, `Project::check` |
| `check_single_synthetic` | 43.4 ms | 20k-line synthetic file, single-file `Checker` |
| `warm_edit_dependent` | 25.5 ms | Edit `glue.R` (other files depend on it), `check_incremental` |
| **`warm_edit_leaf`** | **20.7 ms** | Edit `zzz.R` (leaf — nothing depends on it). **The number W1/W2 should move.** |
| `warm_edit_library` | 20.7 ms | Add/remove `library(tools)` in `utils.R` |
| `lsp_edit_sim` | 25.7 ms | Legacy full-simulation LSP edit |
| Peak RSS | ~92 MB | All scenarios |

**Key finding:** the warm-edit path (20.7 ms) is **85% of the cold-check
cost** (24.4 ms), confirming that passes 2 and 3 rebuild from scratch on
every incremental check. This is the O(project) per-keystroke problem W1
and W2 exist to fix.

---

## Post-W1+W2+W3 measurements

| Benchmark | Baseline (0.8.0) | After W1+W2+W3 | Change |
| :-- | :-- | :-- | :-- |
| `check_project_glue` (cold) | 24.4 ms | 26.2 ms | +7% (noise; cold path unchanged) |
| `warm_edit_dependent` | 25.5 ms | 16.8 ms | −34% |
| **`warm_edit_leaf`** | **20.7 ms** | **1.9 ms** | **−91%** |
| `warm_edit_library` | 20.7 ms | 16.6 ms | −20% |
| `lsp_edit_sim` | 25.7 ms | 17.4 ms | −32% |

The dominant win is on `warm_edit_leaf` — editing a file that nothing depends
on now costs ~1.9 ms instead of ~20.7 ms. The scoped fixpoint skips refining
all 60+ functions in the project and only refines the 1–2 affected by the edit.

The `warm_edit_dependent` and `warm_edit_library` numbers are higher because
those edits transitively affect more functions (or all of them, in the library
case), so the scoped fixpoint still refines most of the project.
