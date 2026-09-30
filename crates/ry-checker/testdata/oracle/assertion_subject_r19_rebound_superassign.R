# oracle: must-warn RY032
`<<-` <- function(name, value) {
  assign("x", c(1L, 2L), envir = parent.frame())
  invisible(value)
}
f <- function(x = 1L) {
  stopifnot(x > 0 && TRUE)
  put <- function(...) NULL
  put <<- function(...) NULL
  if (is.null(x) || x == 1L) TRUE else FALSE
}
vector_error <- tryCatch(f(), error = function(e) conditionMessage(e))
stopifnot(grepl("length = 2", vector_error, fixed = TRUE))
