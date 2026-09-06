# oracle: must-warn RY098
# oracle-claim: RY098
by_type <- function(x = x) base::typeof(x)
by_length <- function(x = x) base::length(x)
by_null <- function(x = x) base::is.null(x)
by_function <- function(x = x) base::is.function(x)
by_invisible <- function(x = x) base::invisible(x)
for (f in list(by_type, by_length, by_null, by_function, by_invisible)) {
  result <- tryCatch(f(), error = identity)
  stopifnot(inherits(result, "error"))
  stopifnot(grepl("promise already under evaluation", conditionMessage(result), fixed = TRUE))
}
unused <- function(x) 1L
stopifnot(identical(unused(stop("not forced")), 1L))
quoted <- function(x = x) base::quote(x)
stopifnot(identical(quoted(), quote(x)))
conditional <- function(flag, x = x) base::typeof(if (flag) x else 1L)
stopifnot(identical(conditional(FALSE), "integer"))
halted <- function(x = x) base::typeof({ base::stop("done"); x })
result <- tryCatch(halted(), error = identity)
stopifnot(identical(conditionMessage(result), "done"))
nested_halt <- function(x = x) base::typeof({ base::identity(base::stop("done")); x })
result <- tryCatch(nested_halt(), error = identity)
stopifnot(identical(conditionMessage(result), "done"))
returned <- function(x = x) base::typeof({ return(1L); x })
stopifnot(identical(returned(), 1L))
early_halt <- function(x = local) { base::stop("done"); base::typeof(x); local <- 1L }
result <- tryCatch(early_halt(), error = identity)
stopifnot(identical(conditionMessage(result), "done"))
default_halt <- function(x = { base::stop("done"); base::typeof(x) }) x
result <- tryCatch(default_halt(), error = identity)
stopifnot(identical(conditionMessage(result), "done"))
default_return <- function(x = { return(1L); base::typeof(x) }) x
stopifnot(identical(default_return(), 1L))
default_replace <- function(x = { x <- 1L; base::typeof(x) }) x
stopifnot(identical(default_replace(), "integer"))
default_conditional_replace <- function(flag, x = { if (flag) x <- 1L else x <- 2L; base::typeof(x) }) x
stopifnot(identical(default_conditional_replace(TRUE), "integer"))
stopifnot(identical(default_conditional_replace(FALSE), "integer"))
default_binary_halt <- function(x = base::stop("done") + base::typeof(x)) x
default_index_halt <- function(x = base::stop("done")[base::typeof(x)]) x
for (f in list(default_binary_halt, default_index_halt)) {
  result <- tryCatch(f(), error = identity)
  stopifnot(identical(conditionMessage(result), "done"))
}
`[.force_fixture` <- function(x, i, ...) 1L
`[[.force_fixture` <- function(x, i, ...) 1L
object <- structure(1L, class = "force_fixture")
ignored_subscript <- function(x = x) object[base::typeof(x)]
ignored_double_subscript <- function(x = x) object[[base::typeof(x)]]
stopifnot(identical(ignored_subscript(), 1L))
stopifnot(identical(ignored_double_subscript(), 1L))
condition_halt <- function(x = if (base::stop("done")) base::typeof(x) else 1L) x
branches_halt <- function(flag, x = { if (flag) base::stop("done") else base::stop("done"); base::typeof(x) }) x
while_halt <- function(x = { while (base::stop("done")) base::typeof(x) }) x
for_halt <- function(x = { for (i in base::stop("done")) base::typeof(x) }) x
for (f in list(condition_halt, while_halt, for_halt)) {
  result <- tryCatch(f(), error = identity)
  stopifnot(identical(conditionMessage(result), "done"))
}
result <- tryCatch(branches_halt(TRUE), error = identity)
stopifnot(identical(conditionMessage(result), "done"))
result <- tryCatch(branches_halt(FALSE), error = identity)
stopifnot(identical(conditionMessage(result), "done"))
dynamic_replace <- function(x = x) { assign("x", 1L); base::typeof(x) }
dynamic_default <- function(x = { assign("x", 1L); base::length(x) }) x
nested_replace <- function(x = x) { y <- (x <- 1L); base::typeof(x) }
operand_replace <- function(x = (x <- 1L) + base::length(x)) x
condition_replace <- function(x = { if ((x <- TRUE)) 1L; base::typeof(x) }) x
promise_replace <- function(x = x, y = { x <- 1L; NULL }) { y; base::typeof(x) }
stopifnot(identical(dynamic_replace(), "integer"))
stopifnot(identical(dynamic_default(), 1L))
stopifnot(identical(nested_replace(), "integer"))
stopifnot(identical(operand_replace(), 2L))
stopifnot(identical(condition_replace(), "logical"))
stopifnot(identical(promise_replace(), "integer"))
quoted_replace <- function(x = x) { `x` <- 1L; base::typeof(x) }
stopifnot(identical(quoted_replace(), "integer"))
replace_in <- function(env) { assign("x", 1L, envir = env); NULL }
opaque_replace <- function(x = x) { replace_in(environment()); base::typeof(x) }
stopifnot(identical(opaque_replace(), "integer"))
