# P38 Architecture Progress — One Analysis Host

## Status

**Partial implementation.** W1–W10 complete. W11–W12 are future work requiring
deep refactoring of the LSP handler layer and checker internals.

## Completed workstreams

### P38-W1: Feature differential tests (commit 78e8722)

Created 6 deterministic `#[ignore]`'d integration tests in
`crates/ry-lsp/tests/p38_feature_diff.rs` that expose findings B2–B5:

**Outcome: file deleted.** B2–B5 were treated as defects to fix, and W6/W7 added
cross-file hover, completion, signature help, go-to-definition, and references by
consulting the background file index. Every one of them passed `""` as the source
text, so their ranges collapsed to 0:0 and none produced correct results. Rather
than repair them, the capabilities were removed: the language server's scope is
the diagnostics `ry check` produces, not a cross-file IDE feature set. B2–B5 are
therefore accepted behaviour, not open defects.

- **B2**: hover/completion are single-file; don't see project functions
- **B3**: go-to-definition uses syntax-name matching, not resolved identity
- **B4**: references/rename omit unopened disk files
- **B5**: signature help lacks parameter info for user-defined functions

Verified RED by running with `--ignored`. These are the executable migration
oracle for W6/W7.

### P38-W2: Bootstrap ry-diagnostics (commit a9410c6)

Created `ry-diagnostics` as an independent external crate:
- Repo: `git@github.com:sims1253/ry-diagnostics.git`
- Tagged v0.1.0 (commit f698622)
- Public interface: `TextSize`, `TextRange`, `RuleId`, `Severity`,
  `Confidence`, `Fix`, `Diagnostic`
- No ry dependency; 9 property/unit tests; MSRV 1.88
- CI for fmt/clippy/test/MSRV
- Conversion adapter in `ry-checker/src/diag_adapter.rs`

**Outcome: reverted.** The crate was never consumed. `diag_adapter.rs` was 24
lines converting `Severity`/`Confidence` between the two vocabularies — the only
two types that happened to be structurally identical. `TextSize`, `TextRange`,
`RuleId`, `Fix`, and `Diagnostic` were never imported, because ry uses
`ry_core::Span { start, end, line, col }` and a differently-shaped `Diagnostic`;
adopting them was the whole job and it was never started. W11 deleted the
adapter as "transitional, no callers" and the dangling workspace dependency was
removed in a06df8b. `ry-core` still defines the original types, so the repo got
no lighter.

The lesson for future extractions: name the consumer that does not depend on the
crate being extracted from, and state acceptance in terms of the consuming code
("N call sites import X", "the duplicate type is deleted") rather than the
extracted artifact's existence. See issue #83.

### P38-W3: Correct lower-layer ownership (commits 23644d4, 802c4a6)

Broke the inverted dependency chain:

**Before:**
```
ry-config → ry-checker (wrong)
ry-workspace → ry-checker (wrong)
```

**After:**
```
ry-core ← ry-config, ry-workspace, ry-checker
ry-config (no ry-checker dep)
ry-workspace (no ry-checker dep)
ry-checker → ry-workspace, ry-config, ry-core (correct downward)
```

Moved:
- `Severity`, `Confidence`, `BaselineDiagnostic`, `SERIALIZED_BINDINGS_UNENUMERABLE`,
  `FFI_PRIMITIVES` to `ry-core`
- `NamespaceMetadata`, `namespace_metadata()`, `attached_packages()`,
  `NATIVE_*_SENTINEL` to `ry-workspace/src/packages.rs`
- `PackageFileKind`, `package_file_kind()` to `ry-workspace/src/file_kind.rs`

Added `ecosystem/check-cargo-edges.py` CI gate to enforce forbidden edges.

### P38-W4: ry-analysis crate skeleton (commit 0817fb2)

Created `crates/ry-analysis` with:
- Stable identities: `WorkspaceId`, `FileId`, `Revision`, `DocumentVersion`
- `Change` enum for atomic/batched input mutations
- `AnalysisHost` with open-over-disk precedence
- 4 unit tests covering revision lifecycle, shadowing, and batch atomicity

### P38-W5: Immutable AnalysisSnapshot (commit 845c1c2)

- `AnalysisSnapshot` captures one Revision for immutable read-only queries
- `QueryResult<T>` enum with Ok/Cancelled/NotFound for cooperative cancellation
- `SnapshotDiagnostic` protocol-free diagnostic data
- `AnalysisHost::snapshot()` and `snapshot_with_diagnostics()` methods
- 3 proptest property tests: live-vs-fresh, snapshot immutability, revision monotonicity
- 7 unit tests for snapshot behavior

### P38-W6: SymbolId and cross-file navigation (commit 844f55f)

- `SymbolId` with kinds (Global, Local, Parameter, LoopVar, Function)
- `SymbolIndex` for cross-file definition/reference lookup
- `build_index_from_file` AST walker extracts all bindings and usages
- `merge_indices` combines per-file indices into project-wide index
- Navigation queries on `AnalysisSnapshot`: `definitions(name)`, `references(name)`
- Cross-file tests verify unopened disk files are included

### P38-W7: Interactive query types (commit 692eec8)

- `HoverInfo`, `CompletionItem` + `CompletionKind`, `SignatureInfo`
- `InlayHint` + `InlayHintKind`
- Protocol-free types for future LSP adapter conversion

### P38-W10: Cache decision and cleanup (commit 7442162)

- Deleted `crates/ry-checker/src/cache.rs` (279 lines of dead code)
- Closed issue #47 as superseded
- `docs/architecture/cache-decision.md` documents the rationale

### P38-W8: Unified diagnostics query (commit e3b3bf8)

- Created `ry-analysis/src/check.rs` with `CheckInput`, `CheckOutput`, `check_project()`
- Wired `ry-cli`'s `run_check_once` through `ry_analysis::check_project` instead
  of inline Project coordination (7 `set_*` methods + `check()`)
- 4 unit tests including cross-file resolution

### P38-W9: Query-engine decision (commit 7319aae)

- `docs/architecture/analysis-query-engine.md` documents the decision
- Keep manual revisioned storage for 0.9
- Salsa migration path documented with rollback triggers

### P39-W4: Catalog adapter (commit 7319aae)

- `ry-analysis/src/catalog_adapter.rs` converts typeshed `FunctionSig` to
  neutral `FunctionSemantics` IR
- Maps EvalMode → Evaluation, ReturnSpec → ReturnRule, PredicateSpec → FlowEffect
- `catalog_from_typeshed()` builds InMemoryCatalog from loaded packages

## Remaining workstreams (future work)

| WS | Description | Effort |
|----|-------------|--------|
| W8 | CLI routes through `ry_analysis::check_project` ✅ | Done |
| W9 | Query-engine decision: manual for 0.9 ✅ | Done |
| W11 | Remove compatibility state | Future |
| W12 | Final acceptance | Future |

## Dependency graph (current)

```
ry-typeshed
ry-core
ry-config ← ry-core
ry-workspace ← ry-core, ry-config, ry-typeshed
ry-checker ← ry-core, ry-config, ry-workspace, ry-typeshed
ry-analysis ← ry-core, ry-checker, ry-config, ry-workspace, ry-typeshed
ry-cli ← all above
ry-lsp ← all above
```
