# oracle: must-pass
# Membership length follows its left operand, and exact guards protect the RHS.
accept <- function(x = NULL) is.null(x) || "value" %in% x
check_na <- function(x) length(x) == 1L && is.na(x)
check_empty <- function(x) 1 == length(x) && x == ""
stopifnot(accept(NULL), accept(c("other", "value")), !accept(character()))
stopifnot(check_na(NA), !check_na(c(NA, NA)), !check_na(logical()))
stopifnot(check_empty(""), !check_empty(c("", "")), !check_empty(character()))
