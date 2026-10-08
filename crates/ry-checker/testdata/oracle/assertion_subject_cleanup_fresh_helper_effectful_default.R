# oracle: must-warn RY032
f <- function(x=1L) { put <- function(env,v={base::assign("x",c(1L,2L),envir=parent.frame());1L}) base::assign("stored",v,envir=env); stopifnot(x > 0 && TRUE); put(base::new.env()); if(is.null(x) || x == 1L) TRUE else FALSE }
e <- tryCatch(f(),error=identity)
stopifnot(inherits(e,"error"),grepl("length = 2",conditionMessage(e),fixed=TRUE))
