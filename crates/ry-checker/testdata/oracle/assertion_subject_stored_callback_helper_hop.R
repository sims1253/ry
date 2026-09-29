# oracle: must-pass
keep <- function(env, action) base::list(action)
install <- function() keep(parent.frame(), base::delayedAssign)
f <- function(x = 1L) {
  install()
  stopifnot(x > 0 && TRUE)
  if (is.null(x) || x == 1L) TRUE else FALSE
}
stopifnot(f())
