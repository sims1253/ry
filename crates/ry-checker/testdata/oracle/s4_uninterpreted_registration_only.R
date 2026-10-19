# oracle: must-pass
# The only `length` method is registered with a `c()` signature, so ry
# cannot tell which classes it answers; the base result is not evidence.
invisible(setClass("A", representation(v = "numeric")))
invisible(setMethod("length", c(x = "A"), function(x) list(k = 1L)))
stopifnot(identical(length(new("A"))$k + 1L, 2L))
