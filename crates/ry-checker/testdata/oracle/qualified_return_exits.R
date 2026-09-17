# oracle: must-pass
# R pins the premise the walker's return arm now shares with the
# divergence view: base::return(...) exits its caller immediately (the
# stop() after it never runs) and delivers its argument as the value.
f <- function() {
  base::return("early")
  stop("unreachable")
}
stopifnot(identical(f(), "early"))
g <- function(x) {
  if (is.null(x)) {
    base::return(NULL)
  }
  x + 1L
}
stopifnot(identical(g(NULL), NULL))
stopifnot(identical(g(1L), 2L))
