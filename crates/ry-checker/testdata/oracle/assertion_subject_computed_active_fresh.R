# oracle: must-pass
f <- function(x = 1L) {
  stopifnot(x > 0 && TRUE)
  (base::identity(base::makeActiveBinding))("x", function() 1L, base::new.env())
  if (is.null(x) || x == 1L) TRUE else FALSE
}
stopifnot(f())
