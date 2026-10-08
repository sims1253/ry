# oracle: must-warn RY032
f <- function(x=1L,xs=integer()) { p <- base::assign; for(i in xs) p <- function(...) NULL; stopifnot(x > 0 && TRUE); p("x",c(1L,2L),envir=environment()); if(is.null(x) || x == 1L) TRUE else FALSE }
e <- tryCatch(f(),error=identity)
stopifnot(inherits(e,"error"),grepl("length = 2",conditionMessage(e),fixed=TRUE))
