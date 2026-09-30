# oracle: must-pass
put <- "delayedAssign"
install <- function() base::invisible(put)
f <- function(x = 1L) {
  install()
  stopifnot(x > 0 && TRUE)
  if (is.null(x) || x == 1L) TRUE else FALSE
}
stopifnot(f())
