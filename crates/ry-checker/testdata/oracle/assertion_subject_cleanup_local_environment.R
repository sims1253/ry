# oracle: must-pass
f <- function(x=1L) { run <- function(cb) print(cb); stopifnot(x > 0 && TRUE); run(environment()); if(is.null(x) || x == 1L) TRUE else FALSE }; f()
