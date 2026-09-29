# oracle: must-pass
f <- function(x = 1L) {
  stopifnot(x > 0 && TRUE)
  put <- base::delayedAssign
  put("x", c(1L, 2L), assign.env = base::new.env(), eval.env = base::new.env())
  if (is.null(x) || x == 1L) TRUE else FALSE
}
stopifnot(f())
