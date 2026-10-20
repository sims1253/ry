# oracle: must-pass
# `base::identity(value)` forwards `wrapper`'s default into base, not into
# the project `identity`.
identity <- function(x = 0L) x + 1L
wrapper <- function(value = "text") base::identity(value)
stopifnot(identical(identity(1L), 2L))
stopifnot(identical(wrapper(), "text"))
