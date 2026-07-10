# oracle: must-warn RY041
# Recycling a shorter vector in arithmetic: c(1,2,3) + c(1,2) recycles
# with a warning and produces the recycled result.
warned <- FALSE
x <- withCallingHandlers(
  c(1, 2, 3) + c(1, 2),
  warning = function(w) {
    warned <<- TRUE
    invokeRestart("muffleWarning")
  }
)
stopifnot(warned, identical(x, c(2, 4, 4)))
