# oracle: must-warn RY032
# The default, explicit NULL, and scalar NA paths are valid. An option can
# replace the scalar default with a vector before the later `||` condition.
guard <- function(integer_as_continuous = 5L) {
  if (!is.null(getOption("modelbased_integer"))) {
    integer_as_continuous <- getOption("modelbased_integer")
  }
  if (is.null(integer_as_continuous) ||
      is.na(integer_as_continuous) ||
      isTRUE(integer_as_continuous)) FALSE else TRUE
}
# `do.call` keeps the calls runtime-real without claiming that these three
# local calls exhaust the package function's possible inputs.
stopifnot(identical(do.call(guard, list()), TRUE))
stopifnot(identical(do.call(guard, list(NULL)), FALSE))
stopifnot(identical(do.call(guard, list(NA)), FALSE))
options(modelbased_integer = c(1L, 2L))
vector_error <- tryCatch(do.call(guard, list()), error = function(e) conditionMessage(e))
stopifnot(grepl("length = 2", vector_error, fixed = TRUE))
