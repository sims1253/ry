# Persistent Scope map experiment

This isolated prototype compares the standard cloned maps with `im::HashMap`
15.1.0 on historical clone baseline `e67fb55`. It changes only `Scope::bindings`
and `Scope::function_aliases`, using a compile-time feature. Marker sets and
reference provenance retain their existing clone behavior. This is a partial
storage experiment, not a production API proposal.

Both modes use the same source and locked dependencies. The `persistent-scope`
feature switches the two public field types in this experiment; ordinary builds
retain their standard map types. No rule or LSP implementation changes are needed
for the checker build. A production change would still need public API review.

The harness uses the four original journal inputs: 1,024 bindings and 24 nested
branches with sparse, dense same-type, or alternating dense writes, plus the
same 240 checker fixtures. It verifies diagnostic equality and records input
hashes. Callgrind counts the whole process, including parsing and formatting;
DHAT reports cumulative allocations and peak live heap. Neither reports wall
clock speedups.

```sh
python3 experiments/persistent-scope/profile.py \
  --out /tmp/ry-persistent-reproduction --tools callgrind dhat
```

The output directory must be new. To reject a costly prototype before profiling
all inputs, add `--workloads dense corpus`. The default runs all four workloads.

The map uses a hash-array mapped trie with shared nodes and mutable operations;
see the [im HashMap documentation](https://docs.rs/im/15.1.0/im/hashmap/struct.HashMap.html).
Clone savings must be weighed against lookup and node-copying costs on ordinary
and dense-write inputs. No journal thresholds or adaptive branch logic are used.

## Screening result: rejected

Measured source: `3ce456c`, with `im` pinned to 15.1.0 in both lockfiles.
The dense and ordinary corpus runs already regress, so the staged experiment
stops here. Sparse and alternating inputs are preserved by the harness but were
not measured. The marker sets and reference provenance were not converted.

| Workload | Standard instructions | Persistent instructions | Change |
| --- | ---: | ---: | ---: |
| Dense, same type | 1,622,272,839 | 1,706,583,364 | +5.20% |
| 240-file corpus | 197,044,917 | 201,874,723 | +2.45% |

| Workload | Standard allocated bytes | Persistent allocated bytes | Change |
| --- | ---: | ---: | ---: |
| Dense, same type | 185,605,760 | 270,489,732 | +45.73% |
| 240-file corpus | 30,995,908 | 39,830,695 | +28.50% |

Dense peak live heap rises from 50,665,773 to 78,956,964 bytes; corpus peak rises
from 12,214,162 to 12,272,956 bytes. Diagnostic reports match byte for byte for
both workloads under both tools. Exact counts, input hashes, binary hashes and
tool versions are in [results.json](results.json). Original raw profiles are in
`/tmp/ry-persistent-profile` in the experiment session.

The feature-enabled checker build and formatting checks passed. Full workspace
and R oracle gates were not run because the screening results reject this
storage proposal. This evidence does not establish semantic equivalence beyond
the compared corpus and synthetic reports, nor does it rule out other persistent
collection designs. It leaves #130 open and makes no production dependency or
public API change.
