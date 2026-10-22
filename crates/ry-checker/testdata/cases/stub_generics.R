# Calls to generics the base typeshed declares in `s3_generics`: a known
# class dispatches to the stub's method, an unknown receiver is opaque.

## case: cut() on a dendrogram returns a list with `$upper`
# no-diag
# The base stub declares cut as an S3 generic, so an unknown receiver
# no longer takes cut.default's integer return (#711, terra's k_means.R).
f <- function(hc, th) cut(stats::as.dendrogram(hc), h = th)$upper

## case: cut() on a known dendrogram dispatches to cut.dendrogram
# no-diag
# cut.dendrogram returns list(upper =, lower =) (#711, dendextend).
g <- function() {
  x <- structure(list(), class = "dendrogram")
  cut(x, h = 1)$lower
}

## case: `$` on cut() of a numeric vector still errors
# expect: RY061
h <- function() cut(c(1, 2, 3), 2)$lower
