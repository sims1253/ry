# oracle: must-warn RY032
# The installer at the end of the first iteration runs before the second
# iteration's assertion, which reads 1L while the later `||` reads a vector.
f <- function(x = 1L) {
  for (i in 1:2) {
    stopifnot(x > 0 && TRUE)
    if (is.null(x) || x == 1L) TRUE else FALSE
    rm("x")
    reads <- 0L
    makeActiveBinding("x", function() {
      reads <<- reads + 1L
      if (reads == 1L) 1L else c(1L, 2L)
    }, environment())
  }
}
vector_error <- tryCatch(f(), error = function(e) conditionMessage(e))
stopifnot(grepl("length = 2", vector_error, fixed = TRUE))
