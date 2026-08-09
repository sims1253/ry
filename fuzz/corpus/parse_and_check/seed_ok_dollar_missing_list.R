# no-diag
# `$` on a plain list with a missing name returns NULL in R (no error);
# RY060 must only fire for data frames, not plain lists.
v <- list(a = 1, b = 2)$missing
w <- list(a = 1)$also_missing
