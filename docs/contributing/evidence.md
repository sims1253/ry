# Evidence for checker changes

Use this short record for semantic, performance, and concurrency changes. It
extends the [contributor rules](../../CONTRIBUTING.md); it does not replace the
required gates or the false-positive bar. Documentation-only edits can simply
say which claims were checked. Copy the [experiment template](experiment-template.md)
into a PR description or a small linked note when a change needs an oracle.

1. State the mechanism and the boundary it claims to change. Name a witness
   that should change, an existing true positive that must remain, and a
   neighboring valid idiom that must stay quiet. Say what result would reject
   the hypothesis before editing the checker.
2. Run the smallest controls on the base and candidate revisions. For R
   semantics, retain the source and `Rscript --vanilla` output. For a speed
   claim, hold the workload and environment fixed; report whether the number
   is a local measurement or an end-to-end result. Headline corpus totals
   cannot establish that individual findings were preserved.
3. Run the targeted tests, then the ordered gates in `CONTRIBUTING.md` and
   relevant real-source or performance checks. Record exact commands, source
   revisions, workload/ledger identities, raw outputs, exit statuses, and
   checks that did not run. A missing tool or an unrun comparison is not a
   pass. Updating an expected value is not evidence that it is correct.
4. Ask a reviewer to replay the decisive claim from those artifacts. Review
   changes to oracle premises, `known-gap` tags, triage labels, benchmark
   workloads, and reference baselines explicitly. A second model's agreement
   or an aggregate green result cannot replace an independent oracle.

For a rejected approach, keep the hypothesis, invalid assumption, smallest
counterexample, base and candidate revisions, evidence link, and reason for
rejection. Link a later fix rather than rewriting the rejected record. Revisit
it when a new proof or boundary resolves the counterexample. The same contract
applies to AI-assisted work: agents may prepare scoped experiments, while
semantic contract changes, baseline or ledger acceptance, and merging remain
review decisions. This grants no extra repository write access.

## Historical example: classed callbacks

This is a **documented historical result**, not a fresh run for this guide.
[Issue #324](https://github.com/sims1253/ry/issues/324) records that the broad
class guard in [PR #318](https://github.com/sims1253/ry/pull/318) fixed genuine
dispatch mistakes and left the 500-package comparison unchanged. Yet frozen
main `373d6a5` reported RY040 for the following, while candidate `92b3673`
did not:

```r
df <- data.frame(a = c("x", "y"), b = c("z", "w"))
lapply(df, function(v) v + 1L)
```

R errors on the character arithmetic. That named true
positive rejects the broad guard despite unchanged aggregate counts. The
issue also records why an exact-data-frame exception alone is insufficient;
the operation and proven callee matter. No benchmark or R control for #324
was rerun here.

[Issue #323](https://github.com/sims1253/ry/issues/323) records a separate
rejected branch-refinement approach: unknown calls and superassignment broke
its assumptions. Its known-gap tags remain premises to review, not evidence
that the rejected patch should be restored.

## Fresh pilot: rejected scalar-flow candidate

The [#605 first-candidate pilot](pilots/605-r1/README.md) retains three tiny R
sources, exact baseline and candidate revisions, raw checker/R outputs, and a
replay script. One candidate false positive and one lost true positive were
found; the third source records an existing baseline gap. These cases were
reproduced against R 4.6.1. The later PR is separate from this rejected
revision. This pilot checks whether another contributor can replay the key
claim without a chat transcript.
