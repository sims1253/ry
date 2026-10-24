## case: a nested replacement through a positional element keeps the element
# no-diag
# `o$groups[[1]]$lr <- 1` writes a field of the first element, not of
# `o$groups` itself (#708, torch's test-ignite.R, forecast's mforecast.R).
f <- function(o) {
  o$groups[[1]]$lr <- 1
  o$groups[[1]]$momentum
}
g <- function(n, fit) {
  out <- list(fc = vector("list", n))
  for (i in seq_len(n)) {
    out$fc[[i]] <- fit(i)
    out$fc[[i]]$series <- "x"
  }
  vapply(out$fc, function(x) x$method, character(1))
}

## case: a nested replacement of a computed element keeps its container a list
# no-diag
# `x$a$b[[i]] <- "z"` replaces one element of the list `x$a$b` (#708).
h <- function(i) {
  x <- list(a = list(b = list(1, 2)))
  x$a$b[[i]] <- "z"
  x$a$b$k
}

## case: a named nested replacement still records the field type
# expect: RY061
k <- function() {
  x <- list(a = list())
  x$a$b <- 1
  x$a$b$c
}

## case: a nested replacement through an atomic element still reports RY061
# expect: RY061
# R evaluates the inner getter `*tmp*`$f on the double `e[1]` and errors.
f <- function() {
  e <- c(1, 2)
  e[1]$f$g <- 2
}

## case: a nested replacement through a data frame column still reports RY061
# expect: RY061
g <- function() {
  df <- data.frame(a = 1)
  df[[1]]$f <- 2
}
