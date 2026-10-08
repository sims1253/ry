f <- function(x) {
 stopifnot(is.null(x) || (x > 0 && { assign("x", c(1L, 2L)); TRUE }))
 if (is.null(x) || x == 1L) TRUE else FALSE
}
f(1L)
