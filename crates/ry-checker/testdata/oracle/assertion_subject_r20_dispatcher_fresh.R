# oracle: must-pass
f <- function(x=1L) {
 stopifnot(x>0 && TRUE)
 d <- base::do.call
 d(base::assign, {
  d <- function(...) NULL
  base::list("x", c(1L,2L), envir=base::new.env())
 })
 if(is.null(x)||x==1L) TRUE else FALSE
}
stopifnot(isTRUE(f()))
