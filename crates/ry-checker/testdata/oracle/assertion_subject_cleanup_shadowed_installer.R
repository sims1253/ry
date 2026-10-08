# oracle: must-pass
delayedAssign <- function(...) NULL
f <- function(x=1L) { stopifnot(x > 0 && TRUE); delayedAssign("x",2L); if(is.null(x) || x == 1L) TRUE else FALSE }; f()
