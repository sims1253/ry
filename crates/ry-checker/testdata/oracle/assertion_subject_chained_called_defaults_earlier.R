# oracle: must-warn RY032
install <- function(env,
  second = function() {
    rm('x', envir = env)
    reads <- 0L
    makeActiveBinding('x', function() { reads <<- reads + 1L; if (reads == 1L) 1L else c(1L, 2L) }, env)
  },
  first = function() second()) { first() }
f <- function(x = 1L) {
  install(environment())
  stopifnot(x > 0 && TRUE)
  if (is.null(x) || x == 1L) TRUE else FALSE
}
vector_error <- tryCatch(f(), error = function(e) conditionMessage(e))
stopifnot(grepl("length = 2", vector_error, fixed = TRUE))
