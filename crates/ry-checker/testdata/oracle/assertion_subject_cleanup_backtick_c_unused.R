# oracle: must-pass
put <- function(env, `c`) base::assign("x",base::c(1L,2L),envir=env)
f <- function(x=1L) { stopifnot(x > 0 && TRUE); target <- environment(); put(base::new.env(),function(...) {base::assign("x",base::c(1L,2L),envir=target);1L}); if(is.null(x) || x == 1L) TRUE else FALSE }
f()
