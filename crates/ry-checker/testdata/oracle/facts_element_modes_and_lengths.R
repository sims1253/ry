# oracle: must-pass
# `for` reads a factor through its levels, so the loop variable is a
# character string, while other classes drop to their storage mode (#861).
# Positional and named subassignment on lists moves the length, and NULL
# deletes (#863). `list(...)` holds as many elements as the caller passed
# (#864). GNU R 4.6.1 verified below.
loop_last <- function(x) {
  for (v in x) NULL
  v
}
stopifnot(identical(loop_last(factor(c("a", "b"))), "b"))
stopifnot(identical(loop_last(ordered(c("a", "b"))), "b"))
stopifnot(identical(loop_last(as.Date("2020-01-01")), 18262))
stopifnot(identical(
  loop_last(structure(1:2, class = c("tagged", "factor"), levels = c("a", "b"))),
  "b"
))
# `structure()` converts double storage to integer for a factor class,
# even one only known at run time.
cls <- "factor"
stopifnot(identical(loop_last(structure(c(1, 2), class = cls, levels = c("a", "b"))), "b"))
l <- list(); for (i in 1:3) l[[i]] <- i
stopifnot(length(l) == 3L)
l <- list(1, 2, 3, 4); l[[2]] <- NULL
stopifnot(length(l) == 3L)
l <- list(1, 2); l[2] <- NULL
stopifnot(length(l) == 1L)
x <- 1:3; x["a"] <- 1L
stopifnot(length(x) == 4L)
x <- 1:3; suppressWarnings(x$a <- 1)
stopifnot(is.list(x), length(x) == 4L)
x <- 1:3; x[2] <- 1L
stopifnot(length(x) == 3L)
rest <- function(...) {
  out <- list(...)
  length(out)
}
stopifnot(rest() == 0L, rest(1, 2, 3) == 3L)
lead <- function(a, ...) {
  out <- list(a, ...)
  length(out)
}
stopifnot(lead(1) == 1L, lead(1, 2, 3) == 3L)
