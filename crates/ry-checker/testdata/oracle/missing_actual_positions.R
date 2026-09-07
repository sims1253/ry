# oracle: must-pass
# An omitted actual occupies its formal while allowing that formal's default.
choose <- function(first = 1L, second = 2L, third = 3L) c(first, second, third)
stopifnot(identical(choose(, second = 5L, 6L), c(1L, 5L, 6L)))
stopifnot(identical(choose(first = , third = 6L), c(1L, 2L, 6L)))
d <- data.frame(value = 1L, other = "x")
stopifnot(identical(d[, "value"], 1L), identical(d[, "other"], "x"))
stopifnot(identical(round(1.25, digits = ), 1))
