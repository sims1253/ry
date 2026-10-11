# expect: RY032
box::use(./modules/tools)
f <- function(x = 1L) {
  stopifnot(x > 0 && TRUE)
  tools$touch(environment())
  if (is.null(x) || x == 1L) TRUE else FALSE
}
