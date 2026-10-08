f <- function(x) {
 mutate <- function() { x <<- c(1L, 2L); TRUE }
 stopifnot(is.null(x) || (x > 0 && mutate()))
 if (is.null(x) || x == 1L) TRUE else FALSE
}
f(1L)
