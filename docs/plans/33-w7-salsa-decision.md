# Plan 33 W7: Salsa Decision Review

## Evaluation date: 2026-08-06. Based on W0–W5 implementation.

### Question 1: What did W0 measure before and after W1–W5?

| Benchmark | Baseline (0.8.0) | After W1+W2+W3 | Change |
| :-- | :-- | :-- | :-- |
| `warm_edit_leaf` | 20.7 ms | 1.9 ms | **−91%** |
| `warm_edit_dependent` | 25.5 ms | 16.8 ms | −34% |
| `warm_edit_library` | 20.7 ms | 16.6 ms | −20% |
| `lsp_edit_sim` | 25.7 ms | 17.4 ms | −32% |
| `check_project_glue` (cold) | 24.4 ms | 26.2 ms | +7% (noise) |

The dominant win is `warm_edit_leaf`: editing a file that nothing depends
on costs 1.9 ms instead of 20.7 ms. The scoped fixpoint (W2) skips
refining all 60+ functions and only refines the 1 affected; the dirty-set
pass 3 (W1) skips re-emitting unchanged files.

### Question 2: How large and legible did the invalidation logic in W1 and W2 turn out to be?

**W1 (dirty-set pass 3): ~60 lines of logic.** The dirty-set computation
in `refine_and_emit` tracks three conditions: content changed, `loaded`
changed, or called function's return type changed. No special cases —
the `loaded` project-wide dependency is modelled explicitly as "re-emit
everything" rather than discovered as a bug. The per-file call-graph
check uses `file_called_fns`, a cached `HashSet<String>` per file, which
is a simple set intersection.

**W2 (fixpoint scope): ~40 lines of logic.** `compute_fixpoint_scope`
builds the affected function set from dirty paths + transitive closure
through the reverse call graph (file-level: if file X calls an affected
function, all functions in X are affected). One iteration loop, no
special cases. S3/S4 method dispatch is conservatively handled by the
caller falling back to full refinement when `loaded` changes.

**Total: ~100 lines of invalidation logic across W1 and W2.** This is
small and legible. No conditional invalidation special cases, no
staleness bugs found during testing.

### Question 3: Were there staleness bugs? How were they found?

**No staleness bugs were found.** The cold-vs-incremental equivalence
property test (`incremental_matches_cold_after_edits`) verifies that
incremental diagnostics match a fresh cold check after a sequence of
random edits. This test passed on the first run for both W1 and W2.

The conservative first-cut approach (re-emit when in doubt, refine
everything when loaded changes) prevented staleness at the cost of some
efficiency. The efficiency loss is acceptable: `warm_edit_dependent`
still improved 34% even with the conservative `loaded` handling.

### Question 4: What would salsa cost now, given what W1–W6 taught about the real query boundaries?

**The query boundaries are now empirically known:**

1. **Pass 1 (collection):** per-file, independently cacheable. Input =
   parsed AST + config. Output = `CollectedFile` (fn_table, return_slots,
   loaded). This is already cached in `collected_files` and is the target
   of the on-disk cache (W5).

2. **Pass 2 (fixpoint):** whole-project refinement. Input = merged
   `FnTable` + `ReturnSlots`. Output = refined `ReturnSlots`. The
   affected-set computation (W2) already scopes this to changed functions
   + transitive callers.

3. **Pass 3 (emission):** per-file, read-only on the shared tables.
   Input = file AST + refined tables. Output = diagnostics. The dirty-set
   (W1) already scopes this to changed/affected files.

**Salsa cost estimate:**

- **AST conversion:** `SourceFile` is `Vec<Stmt>` with no node identity.
  Salsa needs interned, identity-bearing inputs. Converting the AST to
  salsa-tracked entities is the dominant cost — every `Stmt` and `Expr`
  variant would need a salsa struct or an interning layer. This is weeks
  of work and touches every pass.

- **Query definitions:** The three passes map naturally to salsa queries:
  `collect(path) -> CollectedFile`, `fixpoint(project) -> ReturnSlots`,
  `emit(path, tables) -> Vec<Diagnostic>`. But the fixpoint query's
  dependency on the merged `FnTable` (which is a whole-project union)
  would need careful salsa input design to avoid re-running for every
  file change.

- **Invalidation:** Salsa would formalise what W1 and W2 hand-roll. But
  W1 and W2 are only ~100 lines total, and they work. Salsa would replace
  them with ~200+ lines of query definitions + interning + input tracking.
  The complexity trade-off is not clearly favourable.

### Decision: do not adopt salsa.

**The hand-rolled query graph is sufficient.** The invalidation logic is
small (100 lines), legible, correct (no staleness bugs), and fast
(`warm_edit_leaf` improved 91%). The dominant remaining cost is parsing
(W6), not invalidation.

**Re-evaluate when:** (a) the invalidation logic grows past ~300 lines
due to new rules requiring per-rule dependency tracking, or (b) staleness
bugs appear that the cold-vs-incremental property test doesn't catch, or
(c) incremental parsing (W6) forces AST node identity, which is itself
the salsa conversion.
