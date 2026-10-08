# oracle: must-pass
f <- function(x=1L) { put <- function(...) base::assign("x",c(1L,2L),envir=..1); stopifnot(x > 0 && TRUE); put(base::new.env()); if(is.null(x) || x == 1L) TRUE else FALSE }; f()
