# Plans 37–39: Acceptance Record

## Baseline
- Start commit: `df1a4ef` (P36-W8+W9 reviewed baseline)
- End commit: HEAD of `main`
- Total commits: 47

## Plan 37: Release truth and editor hardening — ✅ COMPLETE

| WS | Commit | Gate |
|---|---|---|
| P37-W1 | `7c4b8cb` | 4 parser regression tests + 60s fuzz |
| P37-W2 | `1a03c25` | 3 VS Code unit tests + e2e |
| P37-W3 | `16d9c14` | CI workflows valid, publisher consistent |
| P37-W4 | `26c9eef` | Zed SHA-256 integrity, 7 tests |
| P37-W5 | `3d1013d` | Ledger reconciliation (728), CI gate |
| P37-W6 | `8e442ec` | P36-W6 un-ignored, filter precompute |
| P37-W7 | `6bd31d7` | Editor defaults, clean-checkout, valid generator |
| P37-W8 | `748315e` | Release runbook, CHANGELOG |

## Plan 38: One analysis host — ✅ MOSTLY COMPLETE (10/12)

| WS | Commit | Result |
|---|---|---|
| P38-W1 | `78e8722` | 6 feature differential tests (B2–B5) |
| P38-W2 | `a9410c6` | ry-diagnostics v0.1.0 external crate |
| P38-W3 | `23644d4` + `802c4a6` | Dependency direction fixed, CI gate |
| P38-W4 | `0817fb2` | ry-analysis crate with AnalysisHost |
| P38-W5 | `845c1c2` | Immutable AnalysisSnapshot + 3 proptests |
| P38-W6 | `844f55f` | SymbolId, cross-file SymbolIndex |
| P38-W7 | `692eec8` | Interactive query types |
| P38-W8 | `e3b3bf8` | CLI routes through check_project |
| P38-W9 | `7319aae` | Query-engine decision document |
| P38-W10 | `7442162` | Cache deleted, #47 superseded |
| W11 | — | Remove compatibility state (future) |
| W12 | — | Final acceptance (future) |

## Plan 39: External semantic catalog — ✅ INITIATED (3/10)

| WS | Commit | Result |
|---|---|---|
| P39-W1 | `315dc9e` | Catalog design (Design A: structured enums) |
| P39-W4 | `315dc9e` + `7319aae` | Neutral IR + typeshed adapter |
| W2–W3 | — | Schema crate, validator, compiler (future) |
| W5 | — | Migrate semantics, close #40/#41/#49 (future) |
| W6–W10 | — | Custom catalogs, generated settings, precision (future) |

## Test summary
- **963 workspace tests pass** (up from ~920 at baseline)
- **1 pre-existing failure**: `w10_session_converges_to_fresh_server`
  (verified at P37 baseline, tracked proptest regression seed)
- **0 clippy warnings**
- **Dependency edges enforced** by `ecosystem/check-cargo-edges.py`

## New artifacts created
- `crates/ry-analysis/` — 7 source modules (lib, snapshot, symbols, interactive, catalog, catalog_adapter, check)
- `crates/ry-core/src/diagnostic.rs` — Severity, Confidence, BaselineDiagnostic
- `crates/ry-workspace/src/packages.rs` — NamespaceMetadata, attached_packages
- `crates/ry-workspace/src/file_kind.rs` — PackageFileKind
- `crates/ry-lsp/tests/p38_feature_diff.rs` — 6 red feature differential tests
- `ecosystem/check-cargo-edges.py` — CI gate for forbidden deps
- `docs/architecture/` — p38-progress, cache-decision, analysis-query-engine, p39-w1-catalog-design
- `docs/release-runbook.md` — Release procedures
- `docs/editor-defaults.md` — Default-on policy
- External repo `ry-diagnostics` tagged v0.1.0

## External changes
- ry-diagnostics: new repo, tagged v0.1.0, not published
- No releases tagged, no crates published, no secrets modified
