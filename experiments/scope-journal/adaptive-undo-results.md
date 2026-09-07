# Adaptive journal branches: rejected

Measured source: `fad2ff0`, following the rejected operation-specific undo prototype. The unchanged harness compares clone and adaptive journal modes in the same binary, with one Rayon thread and installed-library discovery disabled. All four input hashes match the original journal experiment. These are whole-process instruction and allocation measurements, not elapsed-time claims.

| Workload | Instruction change | Allocated-byte change |
| --- | ---: | ---: |
| sparse | -47.45% | -66.42% |
| dense | -3.57% | -23.14% |
| alternating | +10.94% | +8.70% |
| corpus | -0.29% | -0.57% |

Dense alternating assignments still regress materially. Conversion reduces the earlier penalty but does not eliminate it, while adding branch ownership and bookkeeping. This design is rejected; no production PR or further threshold tuning is proposed. All four workloads produced byte-identical diagnostics between modes.

The prototype switches at a completed statement when four times the number of distinct names changed by ordinary assignments reaches the summed current capacities of all eight copied maps/sets. Only actual assignment changes count; idempotent assignments do not count, and other mutations conservatively do not advance the threshold. A retained hash set makes distinct counting incremental. Nested branches have separate sets; their merged changes pass through the parent's assignment operation.

Conversion clones the current branch state, gathers merge candidates from its mutation records, and restores the original scope to its saved mark. It then continues inference on the clone. No statement, return collection, or diagnostic is replayed. After conversion, ordinary assignments and metadata setters retain candidate names but do not retain old values. Child snapshots still journal normally. The branch result owns the clone and its candidate set; merging reads only those candidates, without comparing every scope entry.

Marks remain constant-time amortized operations. Under an entry-operation/value-copy cost model, conversion and eventual clone destruction cost O(capacity), charged to the distinct mutations that crossed the threshold. This is not a byte-weighted bound: identifier and alias strings still copy their contents. At this pinned version, RType's variable-sized shapes/signatures/unions and class strings are shared by Arc, while binding provenance values are fixed-size. Spare map capacity counts toward the threshold; very sparse oversized maps stay in journal mode. Scope growth increases the current threshold rather than being hidden by an initial-size estimate.

This prototype adds branch ownership and bookkeeping complexity, and is not a production proposal. Any remaining meaningful dense/corpus regression rejects it. A later candidate would also need current-main Scope fields and the full current semantic gates, rather than relying on this historical pinned harness.

The four journal unit tests passed, including nested conversion, rollback, metadata restoration, repeated changes to one name, and a sparse oversized bindings map. The full workspace suite passed with journal mode enabled. Clippy with warnings denied, formatting checks, and all 17 R oracle tests also passed. A final lint-only edit removes a redundant Option reborrow; measurements refer to the pre-lint commit above.

Reproduce at the measured source commit with `python3 experiments/scope-journal/profile.py --out /tmp/adaptive-undo-reproduction --tools callgrind dhat`. Exact counts, hashes, and tool versions are in `adaptive-undo-results.json`; raw profiles are in `/tmp/ry-adaptive-undo-profile-v1`.
