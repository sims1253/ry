# oracle: must-warn RY032
f <- function(x = 1L) {
  stopifnot(x > 0 && TRUE)
  rm("x", envir = environment())
  base::identity((function() base::makeActiveBinding)())("x", function() c(1L, 2L), environment())
  if (is.null(x) || x == 1L) TRUE else FALSE
}
vector_error <- tryCatch(f(), error = function(e) conditionMessage(e))
stopifnot(grepl("length = 2", vector_error, fixed = TRUE))
