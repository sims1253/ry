# oracle: must-warn RY042
# oracle-claim: RY042
warned <- FALSE
result <- withCallingHandlers(
  factor(c("a", "b")) + 1,
  warning = function(w) {
    warned <<- TRUE
    invokeRestart("muffleWarning")
  }
)
stopifnot(warned, identical(result, c(NA, NA)))

warned <- 0L
results <- withCallingHandlers(
  list(factor(c("a", "b")) + NULL, NULL + factor(c("a", "b"))),
  warning = function(w) {
    warned <<- warned + 1L
    invokeRestart("muffleWarning")
  }
)
stopifnot(warned == 2L, identical(results, list(c(NA, NA), c(NA, NA))))

warned <- FALSE
result <- withCallingHandlers(-factor(c("a", "b")), warning = function(w) {
  warned <<- TRUE
  invokeRestart("muffleWarning")
})
stopifnot(warned, identical(result, c(NA, NA)))
