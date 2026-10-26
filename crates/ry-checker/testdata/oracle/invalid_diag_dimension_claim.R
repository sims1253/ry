# oracle: must-warn RY128
# oracle-claim: RY128
# A length-one numeric `x` is diag()'s matrix dimension, not a
# diagonal entry: a negative or NA scalar errors before any matrix
# exists, while the same value with an explicit `nrow=` is a
# one-element diagonal. GPArotation's Random.Start shipped the
# scalar shape with a documented domain including the failing
# sign (#320); the literal and bound-binding shapes below are the
# provable stand-ins ry checks.
err <- tryCatch({ diag(-1L); "ok" }, error = function(e) conditionMessage(e))
stopifnot(identical(err, "invalid 'nrow' value (< 0)"))
err <- tryCatch({ diag(NA); "ok" }, error = function(e) conditionMessage(e))
stopifnot(identical(err, "invalid 'nrow' value (too large or NA)"))
err <- tryCatch({ diag(nrow = -1L); "ok" }, error = function(e) conditionMessage(e))
stopifnot(identical(err, "invalid 'nrow' value (< 0)"))
err <- tryCatch({ diag(1L, ncol = NA_integer_); "ok" }, error = function(e) conditionMessage(e))
stopifnot(identical(err, "invalid 'ncol' value (too large or NA)"))
stopifnot(identical(diag(-1, nrow = 1L), matrix(-1, 1L, 1L)))
stopifnot(identical(diag(c(-1, -2)), diag(c(-1, -2), nrow = 2L)))
d <- -1L
err <- tryCatch({ diag(d); "ok" }, error = function(e) conditionMessage(e))
stopifnot(identical(err, "invalid 'nrow' value (< 0)"))
