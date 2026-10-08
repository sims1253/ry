# oracle: must-pass
run <- function(cb) { g <- function() base::invisible(cb); g() }; f <- function(x=1L) { run(base::assign); stopifnot(x > 0 && TRUE); if(is.null(x) || x == 1L) TRUE else FALSE }; f()
