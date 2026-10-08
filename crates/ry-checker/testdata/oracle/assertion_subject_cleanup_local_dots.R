# oracle: must-pass
f <- function(x=1L) { put <- function(...) base::sum(...); stopifnot(x > 0 && TRUE); put(1L,2L); if(is.null(x) || x == 1L) TRUE else FALSE }; f()
