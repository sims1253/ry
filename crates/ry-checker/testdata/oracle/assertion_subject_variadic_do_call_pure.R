# oracle: must-pass
install <- function(env, ...) base::do.call(..1, base::list(env))
f <- function(x = 1L) {
 install(environment(), function(env) NULL)
 stopifnot(x > 0 && TRUE)
 if (is.null(x) || x == 1L) TRUE else FALSE
}
stopifnot(f())
