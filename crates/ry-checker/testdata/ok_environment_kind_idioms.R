# no-diag
# Environment values that R accepts. Attributes are shared by reference,
# so any callee may give an environment a class.
setup <- function(env) class(env) <- "foo"
Ops.foo <- function(e1, e2) "dispatched"
e <- new.env()
setup(e)
e + 1
structure(new.env(), class = "foo") + 1
e$x <- 1
e$x + 1
e[["x"]]
c <- new.env()
c(1, 2) + 1
identical(e, globalenv())
# An environment branch may carry a class set by reference, so even a raw
# operand cannot prove the union's arithmetic invalid. (The condition is
# TRUE at runtime; ry cannot tell.)
Ops.tagged <- function(e1, e2) "dispatched"
tag <- function(env) class(env) <- "tagged"
maybe_env <- if (!is.na(Sys.time())) new.env() else 1L
tag(maybe_env)
maybe_env + as.raw(1)
