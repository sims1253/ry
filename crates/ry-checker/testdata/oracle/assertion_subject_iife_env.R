# oracle: must-warn RY032
install <- function(env) {
  target <- (function() env)()
  rm("x", envir = target)
  reads <- 0L
  makeActiveBinding("x", function() {
    reads <<- reads + 1L
    if (reads == 1L) 1L else c(1L, 2L)
  }, target)
}
wrapper <- install
f <- function(x = NULL) {
  wrapper(environment())
  stopifnot(x > 0 && TRUE)
  if (is.null(x) || x == 1L) TRUE else FALSE
}
vector_error <- tryCatch(f(), error = function(e) conditionMessage(e))
stopifnot(grepl("length = 2", vector_error, fixed = TRUE))
