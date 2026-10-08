# oracle: must-warn RY032
put <- function(env, c) base::assign("x",c(1L,2L),envir=env)
f <- function(x=1L) { stopifnot(x > 0 && TRUE); target <- environment(); put(base::new.env(),function(...) {base::assign("x",base::c(1L,2L),envir=target);1L}); if(is.null(x) || x == 1L) TRUE else FALSE }
e <- tryCatch(f(),error=identity)
stopifnot(inherits(e,"error"),grepl("length = 2",conditionMessage(e),fixed=TRUE))
