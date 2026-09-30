# oracle: must-pass
put <- base::assign
f <- function(x = 1L) {
  stopifnot(x > 0 && TRUE)
  put("x", c(1L, 2L), envir = base::new.env())
  if (is.null(x) || x == 1L) TRUE else FALSE
}
stopifnot(f())
