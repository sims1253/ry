# oracle: must-pass
# A literal NA default is stable on first force; the assertion proves it scalar.
f <- function(x=NA) { stopifnot(is.null(x) || length(x)==1L); if(is.null(x) || is.na(x)) TRUE else FALSE }; f()
