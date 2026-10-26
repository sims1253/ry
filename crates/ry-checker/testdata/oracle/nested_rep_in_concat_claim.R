# oracle: must-warn RY127
# oracle-claim: RY127
# `c()` and `append()` splice a list argument's elements in flat, so
# `list(rep(v, n))` contributes exactly one element -- the whole
# repeated vector -- while `rep(list(v), n)` contributes `n` flat
# copies of `v`. With a count of 1 the two forms agree, which is the
# rule's silent boundary. readr's `col_spec_standardise()` shipped
# the nested form where flat collector elements were required (#359);
# the flat integer elements below are the provable stand-in ry checks.
cols <- list(1L, 2L)
nested <- c(cols, list(rep(3L, 2L)))
stopifnot(identical(length(nested), 3L))
stopifnot(identical(nested[[3L]], c(3L, 3L)))
flat <- c(cols, rep(list(3L), 2L))
stopifnot(identical(length(flat), 4L))
stopifnot(identical(flat[[3L]], 3L), identical(flat[[4L]], 3L))
appended <- append(cols, list(rep(3L, 2L)))
stopifnot(identical(length(appended), 3L))
stopifnot(identical(appended[[3L]], c(3L, 3L)))
once_nested <- c(cols, list(rep(3L, 1L)))
once_flat <- c(cols, rep(list(3L), 1L))
stopifnot(identical(once_nested, once_flat))
