# oracle: must-pass
f <- function(x=NA) { stopifnot(is.null(x) || length(x)==1L); if(is.null(x) || is.na(x)) TRUE else FALSE }; f()
