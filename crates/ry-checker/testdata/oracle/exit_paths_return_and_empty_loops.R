# oracle: must-pass
# A scope can be left through return(), and a loop over a possibly empty
# sequence can run zero times. Both leave the earlier state as a possible
# exit: the final assignment does not describe every call (#859, #860).
f <- function(c) {
  x <- 1L
  if (c) return(typeof(x))
  x <- "a"
  typeof(x)
}
stopifnot(f(TRUE) == "integer", f(FALSE) == "character")

f2 <- function(n) { x <- 1L; for (i in seq_len(n)) x <- "a"; typeof(x) }
g <- function(n) { x <- 1L; while (n > 0) { x <- "a"; n <- 0 }; typeof(x) }
stopifnot(f2(0) == "integer", f2(2) == "character")
stopifnot(g(0) == "integer", g(1) == "character")

# Over an empty sequence R binds the loop variable to NULL, replacing an
# earlier value; a break does not change that.
h <- function(n) { for (i in seq_len(n)) NULL; i }
h2 <- function(n) { i <- "z"; for (i in seq_len(n)) NULL; i }
b <- function(n) { i <- "z"; for (i in seq_len(n)) if (i > 1) break; i }
stopifnot(is.null(h(0)), identical(h(2), 2L))
stopifnot(is.null(h2(0)), identical(h2(2), 2L))
stopifnot(is.null(b(0)), identical(b(3), 2L))

# A loop over 1:3 always runs, so its final state is the body's. So does a
# loop over c(1L, y), which has at least one element.
k <- function() { x <- 1L; for (i in 1:3) x <- "a"; c(typeof(x), typeof(i)) }
stopifnot(identical(k(), c("character", "integer")))
k2 <- function(y) { x <- 1L; for (i in c(1L, y)) x <- "a"; typeof(x) }
stopifnot(k2(NULL) == "character", k2(2:3) == "character")
