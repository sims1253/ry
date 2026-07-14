# no-diag
# Real-world shapes that RY095 and RY096's dots branch wrongly flagged
# in 0.4.0. R parses `!x == y` as `!(x == y)` (unary `!` binds looser
# than comparison), and `hasArg(name)` in a function with `...` is a
# legitimate check for a dots-supplied argument. Both must stay silent.
diffobj_shape <- function(x) {
  !all(diff(x)) == 1L
}

quantreg_shape <- function(object, ..., REML = FALSE) {
  if (!hasArg(edfThresh)) edfThresh <- 1e-4
}
