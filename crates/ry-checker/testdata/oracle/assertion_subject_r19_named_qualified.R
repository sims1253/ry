# oracle: must-warn RY032
f <- function(x = 1L) {
  `base::assign` <- function(...) NULL
  stopifnot(x > 0 && TRUE)
  base::assign("x", c(1L, 2L), envir = {
    `base::assign` <- function(...) NULL
    environment()
  })
  if (is.null(x) || x == 1L) TRUE else FALSE
}
vector_error <- tryCatch(f(), error = function(e) conditionMessage(e))
stopifnot(grepl("length = 2", vector_error, fixed = TRUE))
