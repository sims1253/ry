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


# `x[1] <- list(v)` replaces the first element too.
z <- list(0)
z[1] <- list(list(a = 1))
stopifnot(identical(z[[1]]$a, 1))
single <- function(i) {
  w <- list(0)
  w[i] <- list(list(a = 1))
  w[[1]]$a
}
stopifnot(identical(single(1), 1))

# In a partly named list, `[[1]]` is the first position, the named `a`.
m <- list(a = 1, list(b = 2))
m[[1]] <- list(c = 3)
stopifnot(identical(m[[2]]$b, 2), identical(m$a$c, 3))
