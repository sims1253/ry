# oracle: must-warn RY032
`:` <- function(x, y) integer()
f <- function(x=1L) {
  p <- base::assign
  for (i in 1:2) p <- function(...) NULL
  stopifnot(x > 0 && TRUE)
  p("x", c(1L, 2L), envir=environment())
  if (is.null(x) || x == 1L) TRUE else FALSE
}
e <- tryCatch(f(), error=identity)
stopifnot(inherits(e, "error"), grepl("length = 2", conditionMessage(e), fixed=TRUE))
