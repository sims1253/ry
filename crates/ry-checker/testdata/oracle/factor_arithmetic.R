# oracle: must-warn RY042
warned <- FALSE
result <- withCallingHandlers(
  factor(c("a", "b")) + 1,
  warning = function(w) {
    warned <<- TRUE
    invokeRestart("muffleWarning")
  }
)
stopifnot(warned, all(is.na(result)))
