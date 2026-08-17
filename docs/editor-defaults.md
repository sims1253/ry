# Editor-safe defaults — evidence and policy

## Overview

The default editor configuration (`minConfidence: "low"`, default rule set)
is designed to be safe for untrusted workspaces and broad real-world code.
This document records the corpus evidence behind those defaults and the
policy decisions for each rule.

## Corpus baseline (Plan 34, reconciled P37-W5)

| Metric | Value |
| :-- | --: |
| Total findings | 728 |
| True positives | 37 |
| False positives | 691 |
| Overall precision | 5.08% |
| `R/` precision | 4.20% |

The corpus is dominated by RY010 (unbound variable) false positives from
upstream package bindings, generated code, and test fixtures.

## Default profile policy

The default-enabled rules use the ordinary ry configuration/filter seam
(no client-only suppression), preserving CLI/LSP parity.

| Rule | Default | Verdict | Evidence |
| :-- | :-- | :-- | :-- |
| RY010 (unbound variable) | Enabled | **Keep** | 4 TP in production source; dominant FP source but TP are real bugs |
| RY020 (type mismatch) | Enabled | **Keep** | High signal in production source |
| RY030 (scope error) | Enabled | **Keep** | High signal |
| RY040 (invalid operation) | Enabled | **Keep** | High signal |
| RY090 (partial argument name) | Enabled | **Keep** | Consistent, byte-for-byte correct |
| RY032 (test fixture) | Disabled | **Default-off** | 70/70 false positives in test fixtures |
| minConfidence | "low" | **Keep** | Filters zero-confidence heuristics while retaining all corpus TP |

## Precision implications

At `minConfidence: "low"` with the default rule set, the editor shows all
rules with at least low confidence. Some false positives still appear,
particularly for RY010 in packages with dynamic bindings. Users who want
fewer false positives can:

1. Set `minConfidence: "medium"` or `"high"` to filter lower-confidence findings.
2. Use `ry.toml` to disable specific rules per-project.
3. Use baselines to suppress known false positives.

## No client-only suppression

Editor defaults are enforced through the server configuration, not through
client-side filtering. This ensures CLI and LSP produce identical diagnostics
for the same project, preserving the differential test contract.

## Future improvements

Plan 39 (external semantic catalog) targets the dominant RY010 false-positive
sources through catalog-driven package binding resolution. The precision
target for the post-Plan-39 default profile is 50%.
