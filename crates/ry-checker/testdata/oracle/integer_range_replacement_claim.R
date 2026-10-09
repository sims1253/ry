# oracle: must-warn RY119
# oracle-claim: RY119
x <- c(5L, NA_integer_)
x[is.na(x)] <- 1e10
stopifnot(typeof(x) == "double")
warnings <- character()
answer <- withCallingHandlers(
  as.integer(x),
  warning = function(w) {
    warnings <<- c(warnings, conditionMessage(w))
    invokeRestart("muffleWarning")
  }
)
stopifnot(identical(answer, c(5L, NA_integer_)))
stopifnot(length(warnings) == 1L)
stopifnot(grepl("NAs introduced by coercion to integer range", warnings[[1L]], fixed = TRUE))
