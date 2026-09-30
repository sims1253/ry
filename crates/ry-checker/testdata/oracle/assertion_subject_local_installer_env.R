# oracle: must-pass
install <- function() {
  makeActiveBinding("y", function() 1L, base::new.env())
}
f <- function(x = 1L) {
  install()
  stopifnot(x > 0 && TRUE)
  if (is.null(x) || x == 1L) TRUE else FALSE
}
stopifnot(f())
