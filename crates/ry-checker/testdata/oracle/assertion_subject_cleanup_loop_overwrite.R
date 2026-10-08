# oracle: must-pass
f <- function(x=1L) { p <- base::assign; repeat { p <- function(...) NULL; break }; stopifnot(x > 0 && TRUE); p("x",c(1L,2L),envir=environment()); if(is.null(x) || x == 1L) TRUE else FALSE }; f()
