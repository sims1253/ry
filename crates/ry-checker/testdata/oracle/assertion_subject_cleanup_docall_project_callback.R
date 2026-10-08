# oracle: must-warn RY032
run <- function(env,action) action("x",c(1L,2L),envir=env)
f <- function(x=1L) { stopifnot(x > 0 && TRUE); base::do.call(run,base::list(environment(),base::assign)); if(is.null(x) || x == 1L) TRUE else FALSE }
e <- tryCatch(f(),error=identity)
stopifnot(inherits(e,"error"),grepl("length = 2",conditionMessage(e),fixed=TRUE))
