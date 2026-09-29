# oracle: must-pass
install <- function() {
  p <- base::delayedAssign
  p <- function(...) NULL
  base::do.call(p, base::list())
}
f <- function(x=1L) { install(); stopifnot(x>0 && TRUE); if(is.null(x)||x==1L) TRUE else FALSE }
stopifnot(f())
