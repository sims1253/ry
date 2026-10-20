# oracle: must-pass
# The masked `signature("Q")` registers `length` for `A`, so the base
# `length` result is no evidence for `length(new("A"))`.
invisible(setClass("A", representation(v = "numeric")))
signature <- function(...) methods::signature("A")
invisible(setMethod("length", signature("Q"), function(x) "chr"))
stopifnot(identical(paste0(length(methods::new("A")), "!"), "chr!"))
stopifnot(identical(nchar(length(methods::new("A"))), 3L))
