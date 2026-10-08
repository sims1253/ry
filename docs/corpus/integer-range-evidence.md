# Integer coercion range evidence (#569)

Base R's `as.integer()` truncates finite doubles toward zero, then requires an
integer in `[-2147483647, 2147483647]`. `2147483647.9` and
`-2147483647.9` convert without a warning; `2147483648`, `-2147483648`,
and either infinity produce `NA_integer_` with “NAs introduced by coercion to
integer range”. `NaN` and an input `NA` also convert to `NA_integer_`, but do
not introduce a range NA or that warning. The R oracle fixture
[`integer_range_loss_claim.R`](../../crates/ry-checker/testdata/oracle/integer_range_loss_claim.R)
asserts the value and warning; focused controls cover the boundaries and
quiet cases.

The embedded base typeshed already declares `as.integer` with integer mode,
input length, and `na: true`. That declaration says an NA *can* occur; it
cannot establish whether a particular input introduces one. RY119 therefore
uses bounded value evidence on plain numeric literals, simple assignments,
literal defaults, unary negation, and short `c(...)` vectors. A definite
out-of-range element emits the warning and records its new-NA provenance in
the inferred result. Unknown inputs carry only a *possible* new-NA fact and
do not warn. A prior input NA remains distinct. An immediate unshadowed
`x[is.na(x)] <- nonmissing_value` repairs the value fact and suppresses that
cast's warning. Suppression matches that direct cast's diagnostic; warnings
from nested casts remain visible. A proven scalar repair merges the replacement bounds and
promotes integer storage when needed. A repair selecting nothing keeps the
original bounds. Unknown selection, vector replacements, and mixed casts with
unknown surviving bounds discard numeric evidence, as do arbitrary indexed
writes.

The founding sources are [readxl `standardise_limits()` at pin
`47f8aeac0a99eee6c6db2d64ead2225e5e3ae4af`](https://github.com/tidyverse/readxl/blob/47f8aeac0a99eee6c6db2d64ead2225e5e3ae4af/R/read_excel.R#L319)
and [haven `is_integerish()` at pin
`f067fb27e436bc1207e8424f50df90ed9d5acc3a`](https://github.com/tidyverse/haven/blob/f067fb27e436bc1207e8424f50df90ed9d5acc3a/R/haven-stata.R#L235).
The pinned readxl source, evaluated in an isolated R environment with its
installed namespace as the dependency parent, returns `max_row = NA` and the
range warning for `standardise_limits(NULL, 0, 1e10, TRUE)`. The pinned haven
source similarly returns `NA` with the range warning for
`is_integerish(c(1e10))`. These are R-side semantic checks; the checker never
executes inspected project code.

The original package source remains quiet under RY119 because readxl's
`n_max` and haven's `x` are open-world parameters. The checker does not
propagate a concrete caller argument into either body. A reduced readxl-shaped
case with `limits <- c(0, 1e10); limits[is.na(limits)] <- -1L;
as.integer(limits)` has the necessary proof and warns. Finiteness alone and
unknown doubles are not range proofs. The exact-pin ecosystem comparison
against the reviewed #601 base covers both committed manifests (94 entries,
77 distinct source trees, each `R/` and package root) and has zero changed
diagnostic reports. The corpus ledgers therefore require no identity edits.

Downstream condition analysis can query
`RType::coercion_new_na() -> NewNaProvenance`. `ProvenOnly` means every
element is an NA created by a modeled coercion, `ProvenContains` means at
least one is, `Possible` is uncertainty, and `None` carries no such evidence.
The condition rule in #354 should require a length-one `ProvenOnly` value for
a definite coercion-produced-NA claim; RY119 itself does not emit a second
condition diagnostic. Arithmetic beyond unary negation, general subsetting,
custom coercion methods, and open-world parameters are outside this value
proof and remain conservative.
