# Scope journal experiment

The journal prototype stays outside production. It cuts work on wide scopes with
sparse branch writes, but dense branches that alternate inferred types remain
slower. The ordinary fixture corpus gains too little to justify the extra state
and mutation paths. [Issue #130](https://github.com/sims1253/ry/issues/130) remains
open.

This follows the [#225 investigation](https://github.com/sims1253/ry/issues/130#issuecomment-5560333325),
which retained cheaper branch merging and rejected whole-map copy-on-write.
This experiment keeps the public `Scope` maps and journals mutations instead.
A snapshot stores the log position and scalar flags; rollback replays the log.
Branch merging reads mutation-derived deltas rather than scanning full maps.
Clones start with an empty journal. Binding markers, aliases, reachability and
reference provenance are included.

The first version saved sparse allocations but added about 22% dense instructions.
Moving old types into undo entries, using smaller marker records and borrowing
branch views reduced that cost only slightly. The final version also skips
assignments that leave every tracked binding fact unchanged. That helps repeated
writes of the same type; the alternating-type control exposes the remaining cost.

## Measurement

The [final source and harness](https://github.com/sims1253/ry/tree/6cb70fa98b109edf70b7d384b8bd1be73a944443/experiments/scope-journal)
are pinned at `6cb70fa98b109edf70b7d384b8bd1be73a944443`, based on `e67fb55`.
Only statement `if` branches use the experiment; other scope clones remain.
The same release binary runs with
`RY_SCOPE_JOURNAL=0` and `1`, one Rayon thread and installed R libraries disabled.
Rust is 1.98.1; Valgrind is 3.25.1 on Linux x86-64.

Each synthetic file has 1,024 bindings and 24 nested branches. Sparse branches
write one name each. Dense branches write every name; the alternating variant
switches between integer and character at each depth. The corpus runs each of
240 top-level checker fixtures independently. Parsing occurs before the timed
check, but Callgrind counts the whole process, including parsing and diagnostic
formatting. These are instruction counts, not wall-time speedups or the fixed
package instruction ledger.

| Workload | Clone instructions | Journal instructions | Change |
| :-- | --: | --: | --: |
| Sparse writes | 199,206,416 | 104,834,824 | -47.37% |
| Dense, same type | 1,631,017,829 | 1,583,017,500 | -2.94% |
| Dense, alternating types | 1,831,214,239 | 2,329,137,758 | +27.19% |
| 240-file corpus | 198,082,220 | 197,405,701 | -0.34% |

| Workload | Clone allocations (MB) | Journal allocations (MB) | Change |
| :-- | --: | --: | --: |
| Sparse writes | 67.54 | 22.56 | -66.60% |
| Dense, same type | 185.61 | 143.62 | -22.62% |
| Dense, alternating types | 240.46 | 295.90 | +23.06% |
| 240-file corpus | 30.99 | 30.76 | -0.74% |

Sparse peak live heap falls from 23.70 to 10.82 MB; alternating dense peak falls
from 50.67 to 43.72 MB. Corpus peak stays at 12.21 MB. Exact totals, input hashes
and settings are in [the recorded results](scope-journal-experiment.json).

The reports match byte for byte between modes. Small count differences between
runs remain possible because of randomized hashing and paths. The disabled mode
still contains dormant journal plumbing, so these numbers compare strategies
inside the prototype, not two released versions.

The prototype passed workspace tests with the journal enabled, including scope
and reference-fact coverage, plus Clippy, rustfmt and the full R oracle matrix.
A nested-snapshot test checks marker/scalar rollback and independent clone logs.

## Reproduce

Fetch the experiment branch and use a separate checkout:

```sh
git fetch origin experiment/scope-journal
git worktree add --detach /tmp/ry-scope-journal 6cb70fa98b109edf70b7d384b8bd1be73a944443
cd /tmp/ry-scope-journal
python3 experiments/scope-journal/profile.py \
  --out /tmp/ry-scope-profile --tools callgrind dhat
```

The output directory must be new. The script builds with locked dependencies,
records input SHA-256 hashes and tool versions, preserves raw profiles, and
checks diagnostic equality. `results.json` contains exact allocation and
instruction totals. DHAT totals are cumulative allocations; its peak is live
heap, not process RSS.

A future design should improve dense writes and ordinary workloads before it
replaces the current storage. Persistent collections would need their own
lookup-cost and public-API review; they were not measured here.

## Follow-up experiments

Two further prototypes reduced the dense-write penalty but did not remove it.
Neither is a production candidate.

The [operation-specific records](https://github.com/sims1253/ry/blob/ef88b3defe6f8daddf8cc42767779d3535e8439a/experiments/scope-journal/operation-undo-results.md)
save metadata returned by mutations instead of looking it up again. The
[adaptive version](https://github.com/sims1253/ry/blob/0a9923071d40cc19ee18c4c4cf063dbaf98fa04b/experiments/scope-journal/adaptive-undo-results.md)
switches a branch to cloning after enough distinct bindings change. Conversion
occurs between statements without replaying inference; spare map capacity counts
toward its threshold.

Each row compares journal and clone modes in the same prototype binary, using
the four original inputs. Instruction counts cover the whole process; allocations
are cumulative bytes, not peak memory or elapsed time.

| Prototype | Sparse instructions | Alternating dense instructions | Alternating dense allocations | Corpus instructions |
| :-- | --: | --: | --: | --: |
| Operation-specific records | -48.11% | +15.72% | +23.71% | -0.36% |
| Adaptive branches | -47.45% | +10.94% | +8.70% | -0.29% |

Diagnostics matched between modes, and both prototypes passed workspace tests,
Clippy, formatting, and the R oracle suite. The linked records include exact
counts, source revisions, input hashes, and reproduction commands. They use the
historical experiment base and do not claim to cover later `Scope` fields.

The remaining dense regression and small corpus gain do not justify the extra
branch state. These results leave #130 open without changing production storage.
