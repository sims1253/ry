# oracle: must-pass
# Explicit simplification controls retain list results in R, including when
# the actuals are named and reordered. A nonliteral or forwarded control is
# also not a static proof that the result is atomic.
a <- sapply(1L, function(v) 1L, simplify = FALSE)
b <- sapply(simplify = FALSE, FUN = function(v) 1L, X = 1L)
c <- mapply(SIMPLIFY = FALSE, FUN = function(x) 1L, x = 1L)
d <- mapply(FUN = function(x) 1L, x = 1L, SIMPLIFY = FALSE)
control <- identity(FALSE)
e <- sapply(1L, function(v) 1L, simplify = control)
f <- tapply(1L, 1L, function(v) 1L, simplify = FALSE)
wrapper <- function(...) sapply(1L, function(v) 1L, ...)
g <- wrapper(simplify = FALSE)
stopifnot(
  is.list(a), is.list(b), is.list(c), is.list(d), is.list(e), is.list(f),
  is.list(g),
  identical(a$field, NULL), identical(b$field, NULL),
  identical(c$field, NULL), identical(d$field, NULL),
  identical(e$field, NULL), identical(f$field, NULL),
  identical(g$field, NULL)
)
