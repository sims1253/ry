# oracle: must-warn RY032
f <- function(x = 1L) {
  p <- base::makeActiveBinding
  stopifnot(x > 0 && TRUE)
  rm("x", envir = environment())
  p("x", function() c(1L, 2L),
    env = { p <- function(...) NULL; environment() })
  if (is.null(x) || x == 1L) TRUE else FALSE
}
vector_error <- tryCatch(f(), error = function(e) conditionMessage(e))
stopifnot(grepl("length = 2", vector_error, fixed = TRUE))
