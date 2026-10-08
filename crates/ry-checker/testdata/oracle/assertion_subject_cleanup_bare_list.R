# oracle: must-pass
f <- function(x=1L) { stopifnot(x > 0 && TRUE); base::do.call(base::assign,list("x",c(1L,2L),envir=base::new.env())); if(is.null(x) || x == 1L) TRUE else FALSE }; f()
