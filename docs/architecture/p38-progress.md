# P38 Architecture Progress — One Analysis Host

## Status

**Partial implementation.** W1–W4 complete. W5–W12 are future work requiring
deep refactoring of the LSP handler layer and checker internals.

## Completed workstreams

### P38-W1: Feature differential tests (commit 78e8722)

Created 6 deterministic `#[ignore]`'d integration tests in
`crates/ry-lsp/tests/p38_feature_diff.rs` that expose findings B2–B5:

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

## Remaining workstreams (future work)

| WS | Description | Effort |
|----|-------------|--------|
| W5 | Immutable revisions and query lifecycle (`AnalysisSnapshot`) | High |
| W6 | Resolved symbols (`SymbolId`) and project-aware navigation | High |
| W7 | Project types powering hover/completion/signatures/hints | High |
| W8 | Migrate CLI/LSP diagnostics through one `AnalysisSnapshot` query | Medium |
| W9 | Query-engine decision (Salsa vs manual, with benchmarks) | High |
| W10 | Cache decision and cleanup (#47) | Medium |
| W11 | Remove compatibility state | Medium |
| W12 | Final acceptance | Medium |

## Dependency graph (current)

```
ry-diagnostics (external leaf, v0.1.0)
ry-typeshed
ry-core ← ry-diagnostics (not yet consumed)
ry-config ← ry-core
ry-workspace ← ry-core, ry-config, ry-typeshed
ry-checker ← ry-core, ry-config, ry-workspace, ry-typeshed, ry-diagnostics
ry-analysis ← ry-core, ry-checker, ry-config, ry-workspace, ry-typeshed
ry-cli ← all above
ry-lsp ← all above
```
