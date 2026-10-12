# expect: RY032
# Box calls may change the caller environment, dropping scalar facts.
box::use(./modules/tools[touch])
f <- function(x = 1L) {
  stopifnot(x > 0 && TRUE)
  touch(environment())
  if (is.null(x) || x == 1L) TRUE else FALSE
}
