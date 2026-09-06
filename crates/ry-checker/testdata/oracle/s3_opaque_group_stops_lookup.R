# oracle: must-pass
`+.foo` <- function(e1, e2) "wrong"
x <- structure(data.frame(a = 1), class = c("data.frame", "foo"))
y <- x + 1
stopifnot(identical(y$a + 1, 3))
z <- 1 + x
stopifnot(identical(z$a + 1, 3))
`-.foo` <- function(e1, e2) "wrong"
w <- -x
stopifnot(identical(w$a + 1, 0))
