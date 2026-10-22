# oracle: must-pass
# `x[[1]] <- value` replaces the first element of a list, so a later
# `x[[1]]$field` reads the new value, not the one `list(0)` recorded.
mk <- function(a) list(dg2 = a, Ng = a)
literal <- function(a) {
  util <- list(0)
  util[[1]] <- mk(a)
  util[[1]]$dg2
}
stopifnot(identical(literal(5), 5))

# A computed index may replace any element, including the first.
computed <- function(a, i) {
  util <- list(0)
  util[[i]] <- mk(a)
  util[[1]]$dg2
}
stopifnot(identical(computed(5, 1), 5))

# `x[[i]] <- NULL` drops an element and shifts the later ones down.
x <- list(0, list(f = 1))
i <- 1
x[[i]] <- NULL
stopifnot(identical(x[[1]]$f, 1))
y <- list(0, list(f = 1))
y[[1]] <- NULL
stopifnot(identical(y[[1]]$f, 1))

