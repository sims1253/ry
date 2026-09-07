# Ordered reference coverage

The [fixed panel](reference-prefix-panel.json) measures the ordered-write and
proven-prefix extensions to schema-2 reference facts. It contains 15 authored
cases and three existing vendored R files. The real files were selected before
measurement and are pinned by source hash. This is a regression panel, not a
representative estimate of R code coverage.

The baseline uses the reference implementation at `26654ce`. Each source was
checked independently with `ry dump-facts case.R --references`, using the same
source for the baseline and candidate binaries. Counts include exported read
records; they do not count all possible runtime reads.

| Source group | Exported reads | Baseline resolved | Ordered-prefix resolved | Gain |
| --- | ---: | ---: | ---: | ---: |
| 15 authored cases | 39 | 4 | 19 | 15 |
| 3 vendored files | 88 | 0 | 0 | 0 |

The vendored files are glue's `R/quoting.R` and purrr's `R/adverb-possibly.R`
and `R/imap.R`. Their reads remain unavailable under the current restrictions.
The gain in this panel comes entirely from the authored cases. It does not
establish useful coverage for general editor navigation.

The panel records the source text of each authored case, repository paths and
hashes for the vendored files, and both sets of counts. The checker test pins
the candidate counts, verifies deterministic capture, and compares types and
diagnostics with reference capture disabled:

```sh
cargo test -p ry-checker fixed_reference_panel_preserves_counts_and_inference
```

For a CLI comparison, write an authored case's `source` to `case.R`, or copy a
vendored file to that path. Run both binaries on it with `dump-facts case.R
--references`. Count file reference records whose `resolution_status` is
`resolved`. Keep the input sources fixed when comparing versions.

The [reference contract](../facts.md#reference-facts-schema-version-2) describes
the remaining statement, promise, and deferred-function barriers.
