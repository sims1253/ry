# oracle: must-warn RY119
# oracle-claim: RY119
warnings <- character()
answer <- withCallingHandlers(
  as.integer(c(0, 1e10)),
  warning = function(w) {
    warnings <<- c(warnings, conditionMessage(w))
    invokeRestart("muffleWarning")
  }
)
stopifnot(identical(answer, c(0L, NA_integer_)))
stopifnot(length(warnings) == 1L)
stopifnot(grepl("NAs introduced by coercion to integer range", warnings[[1L]], fixed = TRUE))
