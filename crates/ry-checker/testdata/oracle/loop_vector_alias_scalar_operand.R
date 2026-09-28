# oracle: must-warn RY032
# The first loop iteration receives an unclassed length-two vector through
# a local alias, even though a later iteration can replace it with a scalar.
consume <- function(xs) {
  original <- c(1L, 2L)
  alias <- original
  for (i in xs) {
    if (alias == 1L && TRUE) i
    alias <- 1L
  }
}
vector_error <- tryCatch(consume(1L), error = function(e) conditionMessage(e))
stopifnot(grepl("length = 2", vector_error, fixed = TRUE))
