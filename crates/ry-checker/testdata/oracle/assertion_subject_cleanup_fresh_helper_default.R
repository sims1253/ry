# oracle: must-pass
f <- function(x=1L) { put <- function(env,e2=env) base::assign("x",c(1L,2L),envir=e2); stopifnot(x > 0 && TRUE); put(base::new.env()); if(is.null(x) || x == 1L) TRUE else FALSE }; f()
