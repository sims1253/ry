# oracle: must-pass
# Unrelated group methods do not prevent built-in member fallback.
Summary.other <- function(..., na.rm = FALSE) 99L
Math.other <- function(x, ...) 99
x <- structure(c(1L, 2L), class = "widget")
stopifnot(sum(x) == 3L, prod(x) == 2, max(x) == 2L, min(x) == 1L)
stopifnot(all(x), any(x), identical(as.numeric(range(x)), c(1, 2)))
stopifnot(identical(as.numeric(abs(x)), c(1, 2)))
stopifnot(identical(as.numeric(sqrt(x)), sqrt(c(1, 2))))
tab <- structure(c(2L, 1L), dim = 2L, class = "table")
stopifnot(sum(tab) == 3L)

# These members also belong to S3 Math, including S4's Math2 subgroup.
cummax.other <- cummin.other <- cumprod.other <- cumsum.other <- function(x, ...) 99
cospi.other <- sinpi.other <- tanpi.other <- function(x, ...) 99
digamma.other <- trigamma.other <- signif.other <- function(x, ...) 99
stopifnot(identical(as.numeric(cummax(x)), c(1, 2)))
stopifnot(identical(as.numeric(cummin(x)), c(1, 1)))
stopifnot(identical(as.numeric(cumprod(x)), c(1, 2)))
stopifnot(identical(as.numeric(cumsum(x)), c(1, 3)))
stopifnot(identical(as.numeric(cospi(x)), c(-1, 1)))
stopifnot(identical(as.numeric(sinpi(x)), c(0, 0)))
stopifnot(identical(as.numeric(tanpi(x)), c(0, 0)))
stopifnot(identical(as.numeric(digamma(x)), digamma(c(1, 2))))
stopifnot(identical(as.numeric(trigamma(x)), trigamma(c(1, 2))))
stopifnot(identical(as.numeric(signif(x)), c(1, 2)))

Math.widget <- function(x, ...) .Generic
members <- c(getGroupMembers("Math"), getGroupMembers("Math2"))
stopifnot(all(vapply(members, function(name) identical(do.call(name, list(x)), name), logical(1))))
