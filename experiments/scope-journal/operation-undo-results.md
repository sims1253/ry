# Operation-specific undo records: rejected

Measured source: `7d29f72fb4be8fc17db616184e606570609ba113`, based on the original journal experiment `6cb70fa98b109edf70b7d384b8bd1be73a944443`. The unchanged harness compares clone and journal modes in the same binary, with one Rayon thread and installed-library discovery disabled. These are whole-process instruction counts and DHAT allocated bytes, not elapsed-time measurements.

| Workload | Instruction change | Allocated-byte change |
| --- | ---: | ---: |
| sparse | -48.11% | -66.60% |
| dense | -3.68% | -22.59% |
| alternating | +15.72% | +23.71% |
| corpus | -0.36% | -0.73% |

The alternating dense workload still regresses materially. Reduced metadata lookups lower the earlier instruction penalty, but do not justify production integration. No production PR is proposed.

All workload diagnostics were byte-identical between modes. The full workspace test suite, Clippy with warnings denied, formatting checks, two journal unit tests, and all 17 R oracle tests passed with journal mode enabled. Changed-name deltas still derive only from mutation records, without a full-scope scan.

Reproduce with `python3 experiments/scope-journal/profile.py --out /tmp/operation-undo-reproduction --tools callgrind dhat` at the measured source commit. Exact counts, input hashes and tool versions are in `operation-undo-results.json`; original raw profiles are in `/tmp/ry-operation-undo-profile-v1`. This prototype predates later Scope fields and makes no claim about their production integration.
