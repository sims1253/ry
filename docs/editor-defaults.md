# Editor defaults

The editor uses `ry.minConfidence: "low"` and the default rule set. This
document records the corpus evidence and policy behind those choices.
See the [extension guide](../editors/code/README.md) for setup and settings.

## Corpus baseline

The historical 0.9 corpus audit is recorded in
[docs/corpus/0.9-release-evidence.md](corpus/0.9-release-evidence.md):
709 finding records, 43 true positives, 666 false positives, 6.06% overall
precision. The corpus is dominated by RY010 (unbound-variable) false
positives from imported, generated, and data bindings that exist at
runtime.

## Default profile policy

The CLI and language server use the same rule configuration and filters. Every rule
is enabled by default except RY003 (numeric-condition). `ry.minConfidence`
stays `"low"`, which includes all three confidence tiers.

The table below is a curated subset of the registry, not the full rule
list. Codes, names, severities, and defaults mirror
`crates/ry-checker/src/rules.rs`. Verdicts and corpus counts come from
[docs/corpus/rule-evidence-0.9.md](corpus/rule-evidence-0.9.md).

| Rule | Severity | Default | Verdict | Evidence |
| :-- | :-- | :-- | :-- | :-- |
| RY003 (numeric-condition) | info | Disabled | Default-off | 0 corpus findings. Valid claim, but style advice. |
| RY010 (unbound-variable) | warning | Enabled | Keep | 4 true positives / 472 false positives. Largest source of false positives; also catches real bugs. |
| RY020 (unary-minus-type) | error | Enabled | Keep | 0 true positives / 0 false positives in the corpus. Verified by an oracle fixture; scalar parameter defaults can trigger it. |
| RY030 (invalid-comparison) | error | Enabled | Keep | 0 true positives / 1 false positive. The false positive comes from a typeshed coverage gap. |
| RY032 (scalar-logical-length) | warning | Enabled | Keep | 1 true positive / 47 false positives. Fires on non-literal parameter-dependent expressions. |
| RY040 (invalid-arithmetic) | error | Enabled | Keep | 0 true positives / 23 false positives. False positives come from typeshed coverage gaps. |
| RY090 (unknown-argument) | warning | Enabled | Keep | 0 true positives / 4 false positives. Valid syntactic claim. |

## Precision implications

At `ry.minConfidence: "low"` with the default rule set, the editor includes all
confidence tiers for enabled rules. Some false positives still appear,
particularly for RY010 in packages with dynamic bindings. To reduce them:

1. Set `ry.minConfidence: "medium"` or `"high"` to filter lower-confidence findings.
2. Use `ry.toml` to disable specific rules per-project.
3. Use baselines to suppress known false positives.

## No client-only suppression

The server applies editor settings before publishing diagnostics. The CLI
and language server produce the same findings when they use the same
configuration and source contents. Differential tests check this contract.
