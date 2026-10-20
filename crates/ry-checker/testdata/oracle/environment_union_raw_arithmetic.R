# oracle: must-pass
# A union with an environment branch can dispatch an Ops method through a
# class attached by reference, even against a raw operand, so ry must not
# prove `u + as.raw(1)` invalid from the integer branch alone (#648).
Ops.tagged <- function(e1, e2) "dispatched"
tag <- function(env) class(env) <- "tagged"
u <- new.env()
tag(u)
stopifnot(identical(u + as.raw(1), "dispatched"))
# The plain primitive rejects both branches when no class is attached.
stopifnot(inherits(tryCatch(eval(quote(new.env() + as.raw(1))), error = identity), "error"))
stopifnot(inherits(tryCatch(eval(quote(1L + as.raw(1))), error = identity), "error"))
