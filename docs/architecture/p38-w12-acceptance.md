# P38-W12: Final Acceptance Record

## Acceptance criteria (from Plan 38)

- [x] `ry-diagnostics` is public, independently tested, tagged v0.1.0
- [x] config/workspace have no checker dependency (CI-enforced)
- [x] Input changes flow through `AnalysisHost::apply` (ry-analysis crate)
- [x] CLI diagnostics cross one seam (`ry_analysis::check_project`)
- [x] Feature differential tests document all defects (B2–B5)
- [x] Query-engine decision is documented (`analysis-query-engine.md`)
- [x] Cache decision is documented and unsafe cache deleted (#47 superseded)
- [x] Compatibility state removed (re-exports cleaned up)
- [ ] Every advertised feature queries one immutable `AnalysisSnapshot` (W5-W7 provide the types; full LSP migration is W11 future work)
- [ ] Navigation and rename use resolved `SymbolId` across unopened files (types exist; LSP handlers not yet migrated)
- [ ] Latency/memory budgets pass on representative real packages (future measurement)

## Summary

Plan 38 establishes the architectural foundation for unified analysis:
- `ry-analysis` crate with `AnalysisHost`, `AnalysisSnapshot`, `SymbolIndex`
- `ry-diagnostics` external leaf crate tagged v0.1.0
- Correct dependency direction enforced by CI
- CLI diagnostics routed through one entry point
- Immutable snapshot with property-tested live-vs-fresh equivalence
- Cross-file symbol index for project-aware navigation
- Neutral semantic catalog IR bridging typeshed and the checker
- Unsafe cache removed

The LSP handler migration (routing every feature through `AnalysisSnapshot`)
remains future work. The foundation is in place.

## Gates
- 963 workspace tests pass (1 pre-existing convergence failure)
- 0 clippy warnings
- Dependency edges enforced
