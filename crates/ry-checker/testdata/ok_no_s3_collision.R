# no-diag
# `t.test` must NOT be misregistered as an S3 method (generic `t` +
# class `test`). Before Phase 3 item 6, the `[0u8; 64]` prefix scan
# over a too-broad S3_GENERICS list caught `t.` and registered
# `t.test` as a method of generic `t`, which could spuriously suppress
# or misroute dispatch. The curated table + denylist + first-param `x`
# heuristic now skip it; calling `t.test()` resolves as a plain
# function call. R's actual `t.test` is stats::t.test; defining a
# local one shadows it and is a normal (non-S3) function.
t.test <- function(x, t) {
  mean(x)
}
result <- t.test(c(1, 2, 3))
