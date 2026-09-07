# oracle: must-pass
x <- 'bad'
mutate <- function() assign('x', 1L, envir=.GlobalEnv)
out <- switch(1L, {mutate(); x}, 1L)
stopifnot(identical(out+1L, 2L), identical(x+1L, 2L))
x <- 'bad'
object <- structure(1L, class='foo')
`[.foo` <- function(x, ...) {assign('x', 1L, envir=.GlobalEnv); 1L}
out <- switch(1L, {object[1L]; x}, 1L)
stopifnot(identical(out+1L, 2L), identical(x+1L, 2L))
