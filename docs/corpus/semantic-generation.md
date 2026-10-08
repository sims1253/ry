# Bounded R semantic generation

`crates/ry-checker/tests/semantic_generation.rs` tests one audited family:
base-R `if` and `while` conditions built from two literal forms for each
cardinality: `logical(0)`/`logical(0L)`, `TRUE`/`c(TRUE)`,
`FALSE`/`c(FALSE)`, and two-/three-element logical vectors. The first eight
seeds cover both control forms and all four cardinalities. A versioned seed
layout also chooses optional whitespace and one local alias. The alias is assigned once
from a closed literal, read once in a fresh environment, and cannot invoke
dispatch or mutate outside state. Formatting changes only spaces. The family
excludes NSE, reflection, active bindings, custom operators, packages,
unknown effects, and user-supplied source.

The claim is about **condition cardinality**, not whole-program safety. R must
produce its length-zero or length-greater-than-one condition error for an
invalid case, or the exact logical result for a scalar control. ry must place
RY002 at a multi-element `if` condition and RY001 at zero-element or
multi-element `while` conditions. Scalar controls must have neither code.
The case record includes its source, premise, expected code and condition byte
start, R assertion script and bounded process result, checker diagnostics,
generator version, seed, and tool versions.

The normal workspace test includes deterministic grammar, failure-class, and
R-backed controls. If R is absent locally, the R batch explains its skip.
The oracle CI job sets `RY_SEMANTIC_R_REQUIRED=1`, making missing R a hard
failure. To run the same batch locally:

```sh
RY_SEMANTIC_R_REQUIRED=1 \
RY_SEMANTIC_ARTIFACT_DIR=/tmp/ry-semantic-cases \
cargo test -p ry-checker --test semantic_generation small_r_semantic_batch -- --exact --nocapture
```

The batch deliberately hides an observed RY002 once, then reduces that
injected disagreement. A reduction is accepted only when the same R premise,
control form, expected rule, and disagreement fingerprint survive. Fixed
malformed, missing-package, and unrelated-error controls show that another R
failure cannot qualify. `injected-disagreement.json` retains both sources.
Real disagreements also retain original and reduced cases during campaigns;
they do not update accepted snapshots automatically. Review a candidate with
an adjacent quiet control before promoting it to the corpus and R oracle.
Without `RY_SEMANTIC_ARTIFACT_DIR`, records go under the workspace's
`target/semantic-generation` directory. An absolute `CARGO_TARGET_DIR` is
used directly; a relative one is anchored at the workspace root. The explicit
artifact override takes precedence.

The separately budgeted campaign requires a working bubblewrap no-network
namespace. It gives R a read-only host view and only its per-case temporary
directory as writable state. Each process has a 10-second wall limit, 5-second
CPU limit, 1 GiB address-space cap, 16 KiB output-file cap, and 1,024-descriptor
cap; core dumps are disabled. Stdout and stderr are captured and bounded. R is run with `--vanilla`,
base packages only, and temporary home/library directories. This sandbox is
for the audited generator; it does not make arbitrary R safe to run.

```sh
RY_SEMANTIC_CAMPAIGN_SEEDS=64 \
RY_SEMANTIC_ARTIFACT_DIR=/tmp/ry-semantic-campaign \
cargo test -p ry-checker --test semantic_generation sandboxed_campaign -- --ignored --exact --nocapture
```

The count must be 1–64, the complete set of distinct v1 cases. Missing or
blocked bubblewrap fails the campaign;
there is no unconfined fallback. Case records distinguish a semantic
disagreement from malformed source, unsupported runtime premise, missing R,
timeout, crash, output limit, and harness failure. CI uploads the normal
batch's raw records even when the test fails.
