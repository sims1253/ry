# no-diag
# RY126 negative controls: descending all-positive and all-negative
# sequences are legitimate R idioms, `seq2(1, -1)` is empty (rlang
# returns `integer(0)` when `from > to`, it does not descend), a
# `by` that overshoots to exactly 0 never reaches a negative term,
# computed endpoints are unproven, `seq_len` cannot cross zero, and
# a straddling sequence outside index position iterates fine.
# Unknown receivers may dispatch to a `[` method, and `[[` into a
# list can consume a negative step, so both stay silent.
x <- 1:10
x[3:1]
x[-3:-1]
x[1:0]
x[seq(3, 1)]
x[seq(2, -1, by = -2)]
x[rlang::seq2(1, -1)]
x[seq_len(3)]
x[seq_along(x)]
for (i in 1:-1) {
  print(i)
}
computed <- function(v, n) v[1:n]
dbplyr_shape <- function(n) x[seq2(1, n - 1)]
lst <- list(1, 2)
lst[[1:-1]]
masked <- function() {
  seq <- function(a, b) 1L
  y <- c(1, 2, 3)
  y[seq(1, -1)]
}
