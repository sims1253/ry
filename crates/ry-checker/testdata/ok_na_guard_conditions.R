# no-diag
# Guarded NA handling and possibly-NA conditions stay silent (issue #354):
# only a syntactically proven scalar NA is an RY001 condition.
check_value <- function(x) {
  if (!is.na(x)) 1L else 2L
}
check_flag <- function(x) {
  if (isTRUE(x)) 1L else 2L
}
if (any(NA, na.rm = TRUE)) 1L else 2L
value <- NA
if (any(value, na.rm = TRUE)) 1L else 2L
