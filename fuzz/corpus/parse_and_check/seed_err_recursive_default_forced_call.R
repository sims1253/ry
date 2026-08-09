# expect: RY098
# abort() evaluates its message argument; it is not a promise-defusing helper.
f <- function(x = x) rlang::abort(x)
