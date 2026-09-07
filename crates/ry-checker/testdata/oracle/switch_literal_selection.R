# oracle: must-pass
# Literal switch selects one caller expression; alternatives are promises.
stopifnot(identical(switch('a', a=, b=1L, c=stop('unselected')), 1L))
stopifnot(identical(base::switch(2.9, stop('unselected'), 1L), 1L))
stopifnot(identical(switch(E=2L, a=stop('unselected'), EXPR=1L), 1L))
stopifnot(identical(switch(TRUE, 1L, stop('unselected')), 1L))
stopifnot(is.null(switch(FALSE, stop('unselected'))))
stopifnot(is.null(switch(-1, stop('unselected'))))
stopifnot(is.null(switch('absent', a=stop('unselected'))))
stopifnot(is.null(switch('a', a=)))
stopifnot(identical(switch('absent', a=stop('unselected'), 1L), 1L))
x <- 1L
out <- switch('a', a={x <- 'selected'; 1L}, b={x <- FALSE; 2L})
stopifnot(identical(x, 'selected'), identical(out, 1L))
# Character duplicate defaults error before selected evaluation; numeric indexing does not.
stopifnot(inherits(tryCatch(switch('a', a=1L, 2L, 3L), error=identity), 'error'))
stopifnot(identical(switch(1L, a=1L, 2L, 3L), 1L))
stopifnot(inherits(tryCatch(switch(a=1L, EXPR='a'), error=identity), 'error'))
stopifnot(inherits(tryCatch(switch(1L, , 2L), error=identity), 'error'))

stopifnot(inherits(tryCatch(switch(1L, stop('selected'), 'bad'+1), error=identity), 'error'))
f <- function() { switch(1L, {return(1L); stop('unreachable')}, 2L); 3L }
stopifnot(identical(f(), 1L))
