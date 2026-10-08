# oracle: must-pass
f <- function(x=1L) { p <- base::assign; for(i in 1:2) p <- function(...) NULL; stopifnot(x > 0 && TRUE); p("x",c(1L,2L),envir=environment()); if(is.null(x) || x == 1L) TRUE else FALSE }; f()
