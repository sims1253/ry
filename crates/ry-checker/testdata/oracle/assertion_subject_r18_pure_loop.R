# oracle: must-pass
f <- function(x=1L) { stopifnot(x > 0 && TRUE); p <- function(...) NULL; for(i in 1:2) { base::do.call(p, base::list("x", c(1L, 2L), envir=environment())); p <- function(...) NULL }; if(is.null(x)||x==1L) TRUE else FALSE };
stopifnot(f())
