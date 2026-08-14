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
- [ ] Every advertised feature queries one immutable `AnalysisSnapshot` (W5-W7 provide the types; full LSP migration is W11 future work). The cross-file interactive fallbacks that W6/W7 added to the LSP handlers (hover, completion, signature help, go-to-definition, references consulting the on-disk index) have since been **removed** rather than completed: every one shipped passing `""` as the source text, so its ranges collapsed to 0:0 and none produced correct results. The handlers now see open documents only; migrating them onto `AnalysisSnapshot` remains the W11 task.
- [ ] Navigation uses resolved `SymbolId` across unopened files (types exist; LSP handlers not migrated). This was **not** completed: the LSP's project-aware navigation was removed (see above) rather than finished. Rename has been removed from the advertised capabilities — its spelling-based matching is unsafe under R's NSE and dynamic binding — and the cross-file hover/completion/signature-help/definition/references fallbacks were removed for the same correctness reason. All will return together once real resolved-`SymbolId` navigation lands; only open-document navigation remains in scope for this criterion.
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
- 963 workspace tests pass. The one convergence failure recorded here as a
  known baseline was later diagnosed as a race in the w10 test harness
  (the close's clearing publication matched as the reopened document's
  publication), not a checker defect; it is fixed and regression-gated —
  see #81.
- 0 clippy warnings
- Dependency edges enforced
