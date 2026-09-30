# oracle: must-pass
f <- function(x=1L) { stopifnot(x > 0 && TRUE); p <- base::assign; for(i in 1:2) { p <- function(...) NULL; p("x", c(1L, 2L), envir=environment()) }; if(is.null(x)||x==1L) TRUE else FALSE };
stopifnot(f())
