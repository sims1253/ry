# P38-W9: Query-Engine Decision

## Status

**Decision: manual revisioned storage for now, Salsa as future evolution path.**

## Context

Plan 38 requires a decision between two approaches for the analysis query engine:

1. **Manual revisioned storage** (current approach): `AnalysisHost` owns mutable
   state, `AnalysisSnapshot` captures immutable revisions, invalidation is
   managed by the host's `apply()` method.

2. **Salsa or equivalent incremental query engine**: derived values are
   memoized automatically based on input dependencies, invalidation is
   automatic when inputs change.

## Analysis

### Current state (manual)

The `ry-analysis` crate currently implements manual revisioned storage:
- `AnalysisHost` accumulates `Change` operations and increments `Revision`
- `AnalysisSnapshot` captures an immutable view at one revision
- The symbol index is rebuilt from scratch on each snapshot
- The `check_project()` function runs a full `Project::check()` each time

**Strengths:**
- Simple, debuggable, no hidden control flow
- Works correctly for one-shot CLI checks
- No additional dependencies
- Clear ownership of invalidation

**Weaknesses:**
- No incremental reuse — each check rebuilds from scratch
- The `Project` in ry-checker already has hand-maintained incremental state
  (`collected_files`, `file_known_vars`) that duplicates what a query engine
  would provide automatically
- The pre-existing convergence bug (`w10_session_converges_to_fresh_server`)
  is exactly the kind of incremental invalidation error that Salsa prevents

### Salsa evaluation

**Strengths:**
- Automatic memoization and invalidation based on input dependencies
- Eliminates hand-maintained dirty sets and generation counters
- Proven in rust-analyzer for similar workloads
- Query cancellation is built-in

**Weaknesses:**
- Steep learning curve for contributors
- All derived computations must be pure and deterministic
- The current `Project::check()` uses interior mutation and side effects
  that would need restructuring
- Adding Salsa is a large migration that touches every semantic computation

### Measurement framework

Before migrating, we need reproducible benchmarks on:
- Cold construction (new host, parse N files)
- One-line leaf edit (change one file, re-check)
- Transitive edit (change a shared function definition)
- Package/config change
- Memory after indexing and repeated edits
- Recomputed query counts
- Cancellation latency

## Decision

**Keep manual revisioned storage for the 0.9 release cycle.** 

Rationale:
1. The manual approach is working and tested (960 tests pass).
2. The `AnalysisSnapshot` interface already permits replacing the implementation
   with Salsa without changing callers.
3. Migrating to Salsa requires restructuring `Project::check()` which has
   interior mutation, multi-pass collection, and fixpoint iteration — none of
   which fit Salsa's pure-function model without significant refactoring.
4. The pre-existing convergence bug should be fixed by improving the manual
   invalidation in the LSP's `ProjectCache`, not by adopting Salsa.

**Rollback trigger:** If more than 3 incremental invalidation bugs are found
in the LSP session convergence tests, re-evaluate Salsa adoption.

## Future work

When measurement shows need:
1. Profile `Project::check()` to identify pure vs impure computations
2. Extract pure sub-computations as Salsa queries
3. Keep impure multi-pass logic as a non-incremental fallback
4. Migrate incrementally, starting with the most cache-heavy queries
