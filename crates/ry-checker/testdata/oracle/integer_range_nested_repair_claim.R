# oracle: must-warn RY119
# oracle-claim: RY119
warnings <- character()
answer <- withCallingHandlers({
  x <- as.integer({
    as.integer(1e10)
    1e10
  })
  x[is.na(x)] <- 0L
  x
}, warning = function(w) {
  warnings <<- c(warnings, conditionMessage(w))
  invokeRestart("muffleWarning")
})
stopifnot(identical(answer, 0L))
stopifnot(length(warnings) == 2L)
stopifnot(all(grepl("NAs introduced by coercion to integer range", warnings, fixed = TRUE)))
