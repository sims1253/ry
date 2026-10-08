# oracle: must-pass
run <- function() { put <- base::delayedAssign; target <- base::new.env(); put("x",1L,assign.env=target) }; f <- function(x=1L) { run(); stopifnot(x > 0 && TRUE); if(is.null(x) || x == 1L) TRUE else FALSE }; f()
