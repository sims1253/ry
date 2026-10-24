# oracle: must-pass
# `x$f[[i]]$g <- v` writes field `g` of one element of `x$f`; it does not
# make `x$f` itself (or its first element) take the type of `v` (#708).
first_element <- function(o) {
  o$groups[[1]]$lr <- 1
  o$groups[[1]]$momentum
}
stopifnot(identical(first_element(list(groups = list(list(momentum = 0.9)))), 0.9))

computed_element <- function(n, fit) {
  out <- list(fc = vector("list", n))
  for (i in seq_len(n)) {
    out$fc[[i]] <- fit(i)
    out$fc[[i]]$series <- "x"
  }
  vapply(out$fc, function(x) x$method, character(1))
}
stopifnot(identical(computed_element(2, function(i) list(method = "ETS")), c("ETS", "ETS")))

# A computed last step replaces one element; the container stays a list.
last_step <- function(i) {
  x <- list(a = list(b = list(1, 2)))
  x$a$b[[i]] <- "z"
  x$a$b$k
}
stopifnot(is.null(last_step(1)))
