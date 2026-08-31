# Editor-safe defaults — evidence and policy

## Overview

The default editor configuration (`minConfidence: "low"`, default rule set)
is designed to be safe for untrusted workspaces and broad real-world code.
This document records the corpus evidence behind those defaults and the
policy decisions behind them.

## Corpus baseline

The 0.9 corpus baseline is measured and gated in
[docs/corpus/0.9-release-evidence.md](corpus/0.9-release-evidence.md):
728 findings, 37 true positives, 691 false positives, 5.08% overall
precision. The corpus is dominated by RY010 (unbound-variable) false
positives from imported, generated, and data bindings that exist at
runtime.

## Default profile policy

The default-enabled rules use the ordinary ry configuration/filter seam
(no client-only suppression), preserving CLI/LSP parity. Every rule is
enabled by default except RY003 (numeric-condition). `minConfidence`
stays `"low"`: it filters zero-confidence heuristics and retains every
corpus TP.

The table below is a curated subset of the registry, not the full rule
list. Codes, names, severities, and defaults mirror
`crates/ry-checker/src/rules.rs`. Verdicts and corpus counts come from
[docs/corpus/rule-evidence-0.9.md](corpus/rule-evidence-0.9.md).

| Rule | Severity | Default | Verdict | Evidence |
| :-- | :-- | :-- | :-- | :-- |
| RY003 (numeric-condition) | info | Disabled | **Default-off** | 0 corpus findings. Valid claim, but style advice. |
| RY010 (unbound-variable) | warning | Enabled | **Keep** | 4 TP / 472 FP. Dominant FP source, but the TP are real bugs. |
| RY020 (unary-minus-type) | error | Enabled | **Keep** | 0 TP / 0 FP in the corpus. Verified claim; lift-reachable through scalar defaults. |
| RY030 (invalid-comparison) | error | Enabled | **Keep** | 0 TP / 25 FP. FPs come from typeshed coverage gaps. |
| RY032 (scalar-logical-length) | warning | Enabled | **Keep** | 1 TP / 47 FP. Fires on non-literal parameter-dependent expressions. |
| RY040 (invalid-arithmetic) | error | Enabled | **Keep** | 0 TP / 23 FP. FPs come from typeshed coverage gaps. |
| RY090 (unknown-argument) | warning | Enabled | **Keep** | 0 TP / 4 FP. Valid syntactic claim. |

## Precision implications

At `minConfidence: "low"` with the default rule set, the editor shows all
rules with at least low confidence. Some false positives still appear,
particularly for RY010 in packages with dynamic bindings. To reduce them:

1. Set `minConfidence: "medium"` or `"high"` to filter lower-confidence findings.
2. Use `ry.toml` to disable specific rules per-project.
3. Use baselines to suppress known false positives.

## No client-only suppression

Editor defaults are enforced through the server configuration, not through
client-side filtering, so CLI and LSP produce identical diagnostics for
the same project (the differential test contract).
