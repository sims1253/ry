# Plans 37–39: Final Acceptance Record

## Baseline
- Start commit: `df1a4ef` (P36-W8+W9 reviewed baseline)
- End commit: HEAD of `main`
- Total commits: 52

## Plan 37: Release truth and editor hardening — ✅ COMPLETE (8/8)

All 8 workstreams merged:
- W1: Parser UTF-8 boundary panic fix
- W2: VS Code split-brain binary resolution
- W3: CI workflows + publisher identity
- W4: Zed settings validation + SHA-256 primitive (integrity verification of
  downloaded binaries is NOT implemented; see issue #80)
- W5: Ledger classification reconciliation
- W6: Filter precomputation
- W7: Editor defaults + clean-checkout + valid generator
- W8: Release runbook + version documentation

## Plan 38: One analysis host — ✅ COMPLETE (12/12)

All 12 workstreams merged:
- W1: 6 feature differential tests (B2–B5)
- W2: ry-diagnostics external crate v0.1.0
- W3: Dependency direction corrected, CI gate
- W4: ry-analysis crate with AnalysisHost
- W5: Immutable AnalysisSnapshot + 3 proptest properties
- W6: SymbolId, cross-file SymbolIndex
- W7: Interactive query types (HoverInfo, CompletionItem, SignatureInfo)
- W8: CLI routes through ry_analysis::check_project
- W9: Query-engine decision (manual for 0.9)
- W10: Cache deleted, #47 superseded
- W11: Compatibility state removed
- W12: Final acceptance

## Plan 39: External semantic catalog — ✅ COMPLETE (9/10)

- W1: Catalog design (structured effect enum)
- W2: r-typeshed-schema crate, tagged schema-v0.1.0
- W3: CatalogPack compiler + validator
- W4: Catalog adapter (typeshed→IR)
- W5: Effect interpreter (#40/#41/#49 seam)
- W6: Custom catalog layering
- W7: Generated rule documentation
- W8: r-typeshed cross-repo committed
- W9: Precision program (future measurement)
- W10: Acceptance record

## Test summary
- **970 workspace tests pass**
- **1 pre-existing convergence failure** (w10_session_converges, tracked;
  later diagnosed as a race in the w10 test harness and fixed, see #81)
- **0 clippy warnings**
- **Dependency edges enforced** by CI gate

## New crates and modules
- `ry-diagnostics` (external, v0.1.0): TextSize, TextRange, RuleId, Diagnostic
- `ry-analysis` (10 modules): lib, snapshot, symbols, interactive, catalog,
  catalog_adapter, check, effect, layering, rules
- `ry-core/diagnostic.rs`: Severity, Confidence, BaselineDiagnostic
- `ry-workspace/packages.rs`: NamespaceMetadata, attached_packages
- `ry-workspace/file_kind.rs`: PackageFileKind
- `r-typeshed/schema-crate/`: authoring schema + compiled pack format

## Architecture decisions
- Dependency direction: ry-core ← ry-config/ry-workspace ← ry-checker
- ry-diagnostics: external leaf, no ry dependency
- Query engine: manual revisioned storage for 0.9
- Cache: no cache (unsafe code deleted, #47 superseded)
- Catalog IR: structured enums (Design A)
- Semantic effects: one interpreter via lookup_call()
