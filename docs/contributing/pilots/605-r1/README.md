# Rejected #605 first candidate: scalar-flow controls

This is a fresh, bounded replay of the first candidate in
[PR #605](https://github.com/sims1253/ry/pull/605), not a test of that PR's
later head. The base is `adad7e4a1ca1ff291e6574cf5eb9117bb1843431`
(#600); the rejected candidate is
`f1b20085fa0d2471eaad87c8c4a9e30205cd7d9b`. The candidate tried to
retain scalar-assertion facts after a guard and diagnose vectors carried
through loops. It assumed the asserted binding stayed unchanged and that a
loop comparison used base `&&`. These assumptions have separate small
counterexamples here.

| Source | R 4.6.1 | Base checker | Candidate checker |
| --- | --- | --- | --- |
| [masked-and.R](masked-and.R) | Exit 0 | `[]`, exit 0 | RY032 at 2:57, exit 0 |
| [predicate-mutation.R](predicate-mutation.R) | Exit 1 at the later logical OR | RY032 at 3:6, exit 0 | `[]`, exit 0 |

The first candidate invents a warning for an R program that succeeds because
`&&` is masked. It also removes an existing warning for a program that errors:
the `assign()` RHS replaces `x` with a vector after the earlier predicate.
The accepted decision for **this revision** was rejection. A later #605
revision may fix these mechanisms; this record does not assess it. The related
local-helper `x <<-` example was already quiet on the base and is not counted
as a newly lost warning. No timing measurement was made.

The [`results/`](results/) directory contains unmodified stdout, stderr, and
exit status for each of the six commands. The observed binaries were
`ry 0.11.0`: base SHA256
`13c958b0ab171a2b5cdaa9eefa6b2dcef9f188f5d5a4322d82c3143993361759`,
candidate SHA256
`120d03a6956a506601cc24ae4f4ab146ec76f221db9d90217975bdee0fa67c59`.
The R runner was `Rscript 4.6.1` with `LC_ALL=C.UTF-8`. Byte-identical
rebuilds are not required; the source revisions and observed diagnostics are
the claim. A different R version or locale may change error wording even when
the semantic result agrees; inspect that difference before changing retained
outputs.

From a ry clone containing both commits, build isolated binaries and replay
the exact sources. Use separate Cargo targets because the two checker sources
have incompatible artifacts:

```sh
git worktree add --detach ../ry-evidence-base adad7e4a1ca1ff291e6574cf5eb9117bb1843431
git worktree add --detach ../ry-evidence-candidate f1b20085fa0d2471eaad87c8c4a9e30205cd7d9b
(cd ../ry-evidence-base && CARGO_TARGET_DIR=../ry-evidence-target-base cargo build --locked -p ry-cli --bin ry)
(cd ../ry-evidence-candidate && CARGO_TARGET_DIR=../ry-evidence-target-candidate cargo build --locked -p ry-cli --bin ry)
docs/contributing/pilots/605-r1/replay.sh \
  ../ry-evidence-target-base/debug/ry \
  ../ry-evidence-target-candidate/debug/ry
```

`replay.sh` runs each fixture with R and both checkers from this directory,
then compares stdout, stderr, and exit status byte-for-byte with the retained
outputs. The original reviewer independently checked the
[first-candidate controls](https://github.com/sims1253/ry/pull/605#issuecomment-5879116902).
The first candidate's required gates were reported green in that review;
this pilot reran only the six commands above.
The lost named RY032 and new false positive therefore falsify the candidate
even if aggregate totals and gates remain green.
