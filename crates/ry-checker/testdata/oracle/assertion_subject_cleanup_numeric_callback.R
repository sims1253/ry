# oracle: must-pass
show <- function(msg) print(msg)
f <- function(x=1L) { show(1L); stopifnot(x > 0 && TRUE); if(is.null(x) || x == 1L) TRUE else FALSE }
f()
