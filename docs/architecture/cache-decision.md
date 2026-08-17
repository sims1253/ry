# Cache Decision (P38-W10, Issue #47)

## Status

**No cache. Issue #47 is superseded.**

## Decision

The unwired disk cache implementation (`crates/ry-checker/src/cache.rs`)
has been deleted. It was never called in production and its format
restored only partial `FnTable` data with `RType::Unknown`, making it
unsafe for reuse.

## Rationale

1. **The existing cache was not safe.** Issue #47 documented that the
   serialized format loses type information, replacing all inferred
   types with `RType::Unknown`. A cache hit would produce different
   diagnostics than a fresh check.

2. **No measured need.** After the incremental checking improvements in
   Plans 34–36 and the precomputed filter system (P37-W6), cold-start
   diagnostics on typical projects are fast enough without persistence.

3. **Correctness over speed.** Plan 38 Decision 8 states: "No production
   cache is wired until a complete, versioned query summary and measured
   need exist." The revisioned query engine that decision pointed to was
   later deleted (no consumer ever used it), so the precondition stands
   as a design requirement to settle before any cache work, not as a
   tracked document.

## Future cache requirements

If a future measurement shows a user-level need, any cache must:

- Cache immutable, complete semantic query outputs (not mutable AST/checker objects)
- Include in the key: ry semantic format version, analysis version, exact source
  content hash, effective config hash, workspace environment hash, typeshed hash
- Use a documented binary format with atomic writes
- Be corruption-safe (corruption = cache miss)
- Have equivalence tests comparing cache hit to fresh computation

## Action taken

- Deleted `crates/ry-checker/src/cache.rs` (279 lines of unwired code)
- Removed `mod cache;` declaration from `crates/ry-checker/src/lib.rs`
- This document records the decision rationale
