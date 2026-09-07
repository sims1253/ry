# Journaled statement branches

Statement `if` analysis now records binding mutations instead of cloning the
whole scope for each arm. Both arms use the same original scope: mark, analyze,
capture their changed facts, and roll back. The merge preserves the existing
inference rules. Function bodies, callbacks, and expression-form `if` retain
their separate scope clones.

## Cost and state

A mark copies the undo-log length and a fixed set of scalar flags. It allocates
nothing and does not visit bindings. Capturing and rolling back a branch visit
its undo entries, not the full binding table. Repeated writes count as repeated
mutations; rollback does not deduplicate or scan unrelated names. Nested branches
replay their own entries, then record their merged changes in the enclosing log.
Bulk invalidation records every affected binding because that operation changes
every binding.

Changed facts use a hashbrown map with the standard library RandomState hasher.
Borrowed entry lookup avoids cloning duplicate keys, and merged types move out
of the delta rather than being cloned and discarded. One empty delta table can
be reused. Before capture, a cache larger than four times the current reserve
bound (with a 16-entry floor) is discarded. This prevents a previous dense branch
from making a later sparse merge scan the old capacity. The bound is at most
proportional to the number of undo entries; metadata-only changes grow the table
as needed. Hash-table operations have their usual expected-time bounds.

Ordinary assignment, narrowing, default parameters, list markers, aliases,
reference definitions, and temporary pipe bindings all record the facts they
replace. Literal function and plain-vector evidence uses canonical names, so
its undo records are separate from raw binding names. Bulk Ops clears preserve
whole tables. These facts are cleared before merging, making the remaining
raw-name merges independent even when `x` and backtick `x` share a canonical name.

Marks also preserve loop ownership, reachability, unknown-effect flags, and
reference blocker causes, spans, and owners. Break/next snapshots keep the current
semantic state and discard journal history. Independent execution scopes clear
the caller's loop ownership. No public Scope fields or lookup signatures changed.

The cloned implementation remains only in a test module. Tests compare complete
scopes, diagnostics, and reference records against it, including all 500 fixture files discovered
recursively under `testdata` (oracle and vendor fixtures included). Production has no environment switch or second branch
implementation.

## Measurement

The baseline is `f6eefb2dfcb473783f85b7d4b2bd38b7ac4eb698`, which already includes
the general empty-metadata optimization. The candidate checker source tree is
`b9602f26e855fcfa9ff62d0a85bf838e16f850bb`. Separate release binaries use the same
registry dependency versions, one Rayon thread, and no installed R libraries.
Rust is 1.96.1; Valgrind is 3.18.1. Counts include parsing and checking.

| Workload | Instructions | Allocated bytes | Peak live heap change |
| --- | ---: | ---: | ---: |
| sparse | 229,963,328 → 115,413,916 (-49.812%) | 71,425,872 → 25,703,431 (-64.014%) | -42.646% |
| dense, same type | 1,723,246,860 → 1,622,702,399 (-5.835%) | 189,984,294 → 146,676,580 (-22.795%) | -12.080% |
| dense, alternating types | 1,840,950,214 → 1,837,317,371 (-0.197%) | 244,833,818 → 235,740,286 (-3.714%) | -14.060% |
| 253 fixture files | 268,500,305 → 267,598,168 (-0.336%) | 37,464,695 → 37,139,965 (-0.867%) | -0.118% |

Three additional dense alternating pairs change instruction counts by
-0.188%, -0.183%, -0.189%. Their diagnostics also match.
The dense alternating CPU difference is small; it is not a general runtime speed
claim. DHAT reports cumulative allocation and peak live heap, not RSS. The sparse
input is the same 1,024-binding, 24-branch input used by the existing
`check_branch_scopes` Criterion benchmark. Each dense branch writes all 1,024
bindings; the alternating case changes their types at each nesting level.

All four workloads produce identical diagnostics under both instruments. A
separate comparison of all 500 audited packages produces byte-identical JSON.
Workspace tests, strict all-target Clippy, formatting, and the complete R oracle
pass. The 684 checker tests include full scope/reference/diagnostic parity against
the test-only cloned implementation. The existing Criterion branch benchmark
also passes in test mode for both 128 and 1,024 bindings. Strict tidyverse and
full-tier Posit ledger checks, including readable message identities, pass.

[Recorded counts and hashes](scope-journal-results.json) pin the measurement.
The earlier [rejected prototypes](scope-journal-experiment.md) used historical
baselines; their percentages are not current-production comparisons.

To reproduce from the candidate checkout with R, Rust, and Valgrind installed:

```sh
git worktree add --detach /tmp/ry-journal-baseline f6eefb2dfcb473783f85b7d4b2bd38b7ac4eb698
python3 experiments/scope-journal/profile.py \
  --baseline-repo /tmp/ry-journal-baseline \
  --out /tmp/ry-journal-profile
cargo bench -p ry-checker --bench performance -- check_branch_scopes --test
```

Use a new output directory. The harness seeds dependency locks from each
checkout, checks that shared dependency versions agree, saves both binaries and
input hashes, and verifies diagnostic identity. Each worktree has its own Cargo
target directory. Cargo's offline build requires dependencies to be cached first.
