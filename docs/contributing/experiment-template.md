# Experiment record (copy when applicable)

**Status:** proposed / accepted / rejected / unresolved. **Scope:** semantic /
performance / concurrency. Link the issue and PR.

- **Hypothesis and boundary:** What mechanism changes? Which calls, paths, or
  workloads are in scope? What observation would falsify the claim?
- **Controls:** Give the smallest intended change, preserved true positive,
  and adjacent quiet case. For R semantics, include the `Rscript --vanilla`
  premise. For performance, distinguish a local measurement from end-to-end
  benefit and name the fixed workload.
- **Revisions and inputs:** Base and candidate commit IDs; binary/toolchain
  identity; fixture, corpus, ledger, baseline, and workload revisions.
- **Results:** Link raw stdout, stderr, exit status, and finding identities
  for each control. Label observations, hypotheses, measurements, and
  unresolved uncertainty separately. Record every required gate as passed,
  failed, or not run, with its command and evidence path.
- **Independent replay:** Who or what reproduced the decisive result from
  these artifacts? Link their command and output. If none, say not run.
- **Decision:** Retain or reject the invariant and explain why. For a
  rejection, state the invalid assumption and smallest counterexample. Name
  what new proof would justify revisiting it.

Review changes to oracle premises, `known-gap` tags, triage labels, benchmark
workloads, and reference baselines explicitly. An updated expectation alone
does not validate a candidate.
