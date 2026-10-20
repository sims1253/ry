# oracle: must-pass
# A registration with a computed generic name may define `length` for `A`,
# so the base `length` result is no evidence for `length(new("A"))`.
invisible(setClass("A", representation(v = "numeric")))
f <- function(x) "four"
for (g in c("dim", "length")) invisible(setMethod(g, "A", f))
stopifnot(identical(length(methods::new("A")), "four"))
stopifnot(identical(nchar(length(methods::new("A"))), 4L))
stopifnot(identical(length(1:3), 3L))
