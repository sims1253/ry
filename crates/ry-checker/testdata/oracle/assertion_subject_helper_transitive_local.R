# oracle: must-warn RY032
install <- function(env) {
  rm("x", envir = env)
  reads <- 0L
  makeActiveBinding("x", function() {
    reads <<- reads + 1L
    if (reads == 1L) 1L else c(1L, 2L)
  }, env)
}
bridge <- function(target) install(target)
f <- function(x = NULL) {
  bridge(environment())
  stopifnot(x > 0 && TRUE)
  if (is.null(x) || x == 1L) TRUE else FALSE
}
vector_error <- tryCatch(f(), error = function(e) conditionMessage(e))
stopifnot(grepl("length = 2", vector_error, fixed = TRUE))
