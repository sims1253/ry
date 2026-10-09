# oracle: must-warn RY032
# Conservative boundary: R succeeds because the installer targets a fresh
# environment, but every installer call drops the scalar fact, as on main.
f <- function(x = 1L) {
  stopifnot(x > 0 && TRUE)
  put <- base::delayedAssign
  put("x", c(1L, 2L), assign.env = base::new.env(), eval.env = base::new.env())
  if (is.null(x) || x == 1L) TRUE else FALSE
}
stopifnot(f())
