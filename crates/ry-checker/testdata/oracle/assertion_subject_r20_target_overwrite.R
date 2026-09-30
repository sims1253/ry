# oracle: must-pass
f <- function(x=1L) {
stopifnot(x>0 && TRUE)
p <- base::assign
base::do.call(p,base::list("x",c(1L,2L), envir={p<-function(...)NULL;environment()}))
if(is.null(x)||x==1L) TRUE else FALSE
}
stopifnot(isTRUE(f()))
