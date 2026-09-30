# oracle: must-pass
f <- function(x = 1L) {
  stopifnot(x > 0 && TRUE)
  (base::identity(base::delayedAssign))("x", 1L, assign.env = base::new.env(), eval.env = base::new.env())
  if (is.null(x) || x == 1L) TRUE else FALSE
}
stopifnot(f())
