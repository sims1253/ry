# oracle: must-pass
f <- function(x = 1L) {
  stopifnot(x > 0 && TRUE)
  put <- base::makeActiveBinding
  put("x", function() c(1L, 2L), base::new.env())
  if (is.null(x) || x == 1L) TRUE else FALSE
}
stopifnot(f())
