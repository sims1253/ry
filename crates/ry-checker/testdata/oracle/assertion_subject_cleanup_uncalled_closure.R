# oracle: must-pass
run <- function() { g <- function() base::assign("x",c(1L,2L),envir=parent.frame()); NULL }
f <- function(x=1L) { run(); stopifnot(x > 0 && TRUE); if(is.null(x) || x == 1L) TRUE else FALSE }; f()
