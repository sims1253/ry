# oracle: must-pass
install <- function(env) base::assign("x",c(1L,2L),envir=env)
run <- function() { install <- function() NULL; install() }
f <- function(x=1L) { run(); stopifnot(x > 0 && TRUE); if(is.null(x) || x == 1L) TRUE else FALSE }; f()
