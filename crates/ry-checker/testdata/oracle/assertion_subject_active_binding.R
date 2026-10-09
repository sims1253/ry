# oracle: must-warn RY032
# The first active-binding read returns one value; the next returns a vector.
# The literal formal default no longer describes the binding.
f <- function(x = NULL) {
  rm(x)
  reads <- 0L
  makeActiveBinding("x", function() {
    reads <<- reads + 1L
    if (reads == 1L) 1L else c(1L, 2L)
  }, environment())
  stopifnot(x > 0 && TRUE)
  if (is.null(x) || x == 1L) TRUE else FALSE
}
vector_error <- tryCatch(f(), error = function(e) conditionMessage(e))
stopifnot(grepl("length = 2", vector_error, fixed = TRUE))
