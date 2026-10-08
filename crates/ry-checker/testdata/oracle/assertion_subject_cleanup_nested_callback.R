# oracle: must-warn RY032
run <- function(env,action) action("x",c(1L,2L),envir=env)
install <- function(env=parent.frame()) { g <- function() run(env,base::assign); g() }
f <- function(x=1L) { install(); stopifnot(x > 0 && TRUE); if(is.null(x) || x == 1L) TRUE else FALSE }
e <- tryCatch(f(),error=identity)
stopifnot(inherits(e,"error"),grepl("length = 2",conditionMessage(e),fixed=TRUE))
