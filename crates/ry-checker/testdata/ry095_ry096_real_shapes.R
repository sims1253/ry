# expect: RY095 RY096
diffobj_shape <- function(x) {
  !all(diff(x)) == 1L
}

quantreg_shape <- function(object, ..., REML = FALSE) {
  if (!hasArg(edfThresh)) edfThresh <- 1e-4
}
