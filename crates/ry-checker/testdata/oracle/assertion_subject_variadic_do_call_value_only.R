# oracle: must-pass
install <- function(env, ...) base::invisible(base::list(base::identity(..1)))
f <- function(x = 1L) {
 install(environment(), function(env) base::delayedAssign("x", { x <- c(1L, 2L); 1L }, assign.env=env, eval.env=env))
 stopifnot(x > 0 && TRUE)
 if (is.null(x) || x == 1L) TRUE else FALSE
}
stopifnot(f())
