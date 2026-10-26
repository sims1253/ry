# oracle: must-warn RY001
# oracle-claim: RY001
# A vector default reaches `switch` without `match.arg`, violating
# its length-one selector requirement on the no-argument call (#323).
choose_alert <- function(type = c("success", "info", "warning", "danger")) {
  switch(type, success = 1L, info = 2L, warning = 3L, danger = 4L)
}
err <- tryCatch(choose_alert(), error = identity)
stopifnot(inherits(err, "error"))
stopifnot(grepl("EXPR must be a length 1 vector", conditionMessage(err), fixed = TRUE))
stopifnot(identical(choose_alert("info"), 2L))
# With match.arg() the same default is the established idiom.
choose_safe <- function(type = c("success", "info")) {
  type <- match.arg(type)
  switch(type, success = 1L, info = 2L)
}
stopifnot(identical(choose_safe(), 1L))
