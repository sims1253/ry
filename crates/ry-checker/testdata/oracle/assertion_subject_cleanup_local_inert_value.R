# oracle: must-pass
f <- function(x=1L) { cb <- function() NULL; run <- function(cb) cb(); stopifnot(x > 0 && TRUE); run(cb); if(is.null(x) || x == 1L) TRUE else FALSE }; f()
