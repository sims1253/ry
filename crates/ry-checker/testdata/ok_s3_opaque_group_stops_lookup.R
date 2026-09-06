# no-diag
`+.foo` <- function(e1, e2) "wrong"
x <- structure(data.frame(a = 1), class = c("data.frame", "foo"))
# Ops.data.frame wins before lookup reaches the later foo class.
y <- x + 1
y$a + 1
z <- 1 + x
z$a + 1
`-.foo` <- function(e1, e2) "wrong"
w <- -x
w$a + 1
