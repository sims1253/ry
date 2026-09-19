# oracle: must-warn RY001
# oracle-claim: RY001
# The NULL component of a find-or-NULL helper's union return is a real
# crash at unguarded call sites (issue #362, the reprex locate_input
# defects): `switch` errors with "EXPR must be a length 1 vector" and a
# comparison condition errors with "argument is of length zero". The
# unguarded sites below are what ry must flag; every shape stays inside
# functions or tryCatch so this file itself does not error.
locate_input <- function(input) {
  if (is.null(input)) {
    return(NULL)
  }
  "path"
}
stopifnot(is.null(locate_input(NULL)), identical(locate_input("path"), "path"))

switch_crash <- function() {
  where <- locate_input(NULL)
  switch(where, path = 1L, 2L)
}
switch_error <- tryCatch(switch_crash(), error = identity)
stopifnot(inherits(switch_error, "error"))
stopifnot(grepl("EXPR must be a length 1 vector", conditionMessage(switch_error)))

condition_crash <- function() {
  where <- locate_input(NULL)
  if (where == "path") 1L else 2L
}
condition_error <- tryCatch(condition_crash(), error = identity)
stopifnot(inherits(condition_error, "error"))
stopifnot(grepl("argument is of length zero", conditionMessage(condition_error)))

# The same helper is safe once guarded; the non-null path runs clean.
guarded <- function(input) {
  where <- locate_input(input)
  if (is.null(where)) {
    return(NA_character_)
  }
  if (where == "path") "path" else "other"
}
stopifnot(identical(guarded(NULL), NA_character_), guarded("path") == "path")
