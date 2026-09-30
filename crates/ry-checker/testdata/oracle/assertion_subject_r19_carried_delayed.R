# oracle: must-warn RY032
f <- function(x = 1L) {
  p <- function(...) NULL
  assign <- base::assign
  for (i in 1:3) {
    p("x", { x <- c(1L, 2L); 1L }, environment())
    p <- assign
    assign <- base::delayedAssign
    stopifnot(x > 0 && TRUE)
    if (is.null(x) || x == 1L) TRUE else FALSE
  }
}
vector_error <- tryCatch(f(), error = function(e) conditionMessage(e))
stopifnot(grepl("length = 2", vector_error, fixed = TRUE))
