# oracle: must-warn RY032
f <- function(x = 1L) {
  p <- base::delayedAssign
  stopifnot(x > 0 && TRUE)
  p("x", { x <- c(1L, 2L); 1L },
    assign.env = { p <- function(...) NULL; environment() },
    eval.env = environment())
  if (is.null(x) || x == 1L) TRUE else FALSE
}
vector_error <- tryCatch(f(), error = function(e) conditionMessage(e))
stopifnot(grepl("length = 2", vector_error, fixed = TRUE))
