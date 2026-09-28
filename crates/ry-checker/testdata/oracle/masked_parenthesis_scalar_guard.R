# oracle: must-warn RY032
# In R, grouped expressions call `(`. This local function returns TRUE
# without evaluating the apparent length check, so the vector reaches `&&`.
`(` <- function(x) TRUE
f <- function(xs) {
  x <- c(1L, 2L)
  for (i in xs) {
    if ((length(x) == 1L)) {
      if (x == 1L && TRUE) i
    }
    x <- 1L
  }
}
vector_error <- tryCatch(do.call(f, list(1L)), error = function(e) conditionMessage(e))
stopifnot(grepl("length = 2", vector_error, fixed = TRUE))
