# oracle: must-pass
f <- function(`\x2b`) 1L + 2L
stopifnot(identical(f(function(...) "ok"), "ok"))
