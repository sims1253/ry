# oracle: must-pass
f <- function(x=1L) { run <- function(cb=function() NULL) cb(); stopifnot(x > 0 && TRUE); run(); if(is.null(x) || x == 1L) TRUE else FALSE }; f()
