# oracle: must-pass
f <- function(x=1L) { put <- function(env,v) base::assign("x",v,envir=env); stopifnot(x > 0 && TRUE); put(base::new.env(),1L); if(is.null(x) || x == 1L) TRUE else FALSE }; f()
