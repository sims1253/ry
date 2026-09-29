# oracle: must-pass
replace <- function() {
  target <- base::new.env()
  assign("x", c(1L, 2L), envir = target)
}
f <- function(x = 1L) {
  stopifnot(x > 0 && TRUE)
  replace()
  if (is.null(x) || x == 1L) TRUE else FALSE
}
stopifnot(f())
