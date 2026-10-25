# oracle: must-warn RY001
# oracle-claim: RY001
# A proven scalar NA is not a valid `if`/`while` condition in R (issue
# #354): a logical NA errors with "missing value where TRUE/FALSE
# needed", including through a comparison over a typed NA. Every shape
# stays inside tryCatch or an uncalled body so this file itself does
# not error.
comparison_error <- tryCatch(if (NA_integer_ > 1L) TRUE else FALSE, error = identity)
stopifnot(inherits(comparison_error, "error"))
stopifnot(grepl(
  "missing value where TRUE/FALSE needed",
  conditionMessage(comparison_error),
  fixed = TRUE
))

if_error <- tryCatch(if (NA) TRUE else FALSE, error = identity)
stopifnot(inherits(if_error, "error"))
stopifnot(grepl(
  "missing value where TRUE/FALSE needed",
  conditionMessage(if_error),
  fixed = TRUE
))

while_crash <- function() {
  while (NA) break
  TRUE
}
while_error <- tryCatch(while_crash(), error = identity)
stopifnot(inherits(while_error, "error"))
stopifnot(grepl(
  "missing value where TRUE/FALSE needed",
  conditionMessage(while_error),
  fixed = TRUE
))

# The guarded forms of the same values run clean.
value <- NA
stopifnot(is.na(value), !isTRUE(value))
stopifnot(identical(any(NA, na.rm = TRUE), FALSE))
