# oracle: must-warn RY098
# oracle-claim: RY098
f_if_true <- function(x = if (TRUE) x else 1L) x
f_if_false <- function(x = if (FALSE) 1L else x) x
f_and <- function(x = TRUE && x) x
f_or <- function(x = FALSE || x) x
f_block <- function(x = { if (TRUE) x; 1L }) x
f_condition <- function(x = if (x) 1L else 2L) x
for (f in list(f_if_true, f_if_false, f_and, f_or, f_block, f_condition)) {
  result <- tryCatch(f(), error = identity)
  stopifnot(inherits(result, "error"))
}
