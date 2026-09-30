# oracle: must-pass
install <- function(env, act = function() {
  rm("x", envir = env)
  reads <- 0L
  makeActiveBinding("x", function() {
    reads <<- reads + 1L
    if (reads == 1L) 1L else c(1L, 2L)
  }, env)
}) { base::invisible(act) }
f <- function(x = 1L) {
  install(environment())
  stopifnot(x > 0 && TRUE)
  if (is.null(x) || x == 1L) TRUE else FALSE
}
stopifnot(f())
