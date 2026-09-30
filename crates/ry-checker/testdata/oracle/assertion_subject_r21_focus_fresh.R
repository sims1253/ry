# oracle: must-pass
put <- function(env) base::assign('x', c(1L,2L), envir=env)
f <- function(x=1L) {stopifnot(x>0&&TRUE);put(base::new.env());if(is.null(x)||x==1L)TRUE else FALSE};
stopifnot(isTRUE(f()))
