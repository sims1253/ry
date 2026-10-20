# oracle: must-pass
# `==`/`!=` between a character value and an integer, logical, or
# integer-valued double value compares the formatted strings, which matches
# the intended answer, so RY033 stays quiet on equality (#699). Ordering is the
# lexicographic hazard RY033 keeps reporting.
flag <- "1"
stopifnot(isTRUE(flag == 1))
stopifnot(identical(c("1", "2") == 1:2, c(TRUE, TRUE)))
simplify <- TRUE
stopifnot(isFALSE(simplify == "array"), isTRUE(simplify != "array"))
stopifnot(isTRUE("TRUE" == TRUE))
stopifnot(isTRUE("9" > 14))
