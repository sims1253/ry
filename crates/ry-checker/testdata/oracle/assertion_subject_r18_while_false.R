# oracle: must-pass
f <- function(x=1L) { stopifnot(x > 0 && TRUE); p <- function(...) NULL; while(FALSE) { p("x", c(1L, 2L), envir=environment()); p <- base::assign }; if(is.null(x)||x==1L) TRUE else FALSE };
stopifnot(f())
