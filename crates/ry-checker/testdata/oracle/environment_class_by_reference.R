# oracle: must-pass
# Environment attributes are shared by reference: a callee can attach a
# class that later selects an Ops method, so ry never proves an
# environment classless (#648).
setup <- function(env) class(env) <- "foo"
Ops.foo <- function(e1, e2) "dispatched"
e <- new.env()
setup(e)
stopifnot(identical(e + 1, "dispatched"))
