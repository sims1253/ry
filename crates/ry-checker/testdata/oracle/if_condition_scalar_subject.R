# oracle: must-pass
# A passed `if` condition was TRUE or FALSE, so a vector parameter errors at
# the first test and never reaches the later `is.null(x) || pred(x)` guard
# (#702).
check_kind <- function(kind = "warning") {
  if (kind == "stop") stop("stopping")
  if (is.null(kind) || kind == "ignore") return(NULL)
  kind
}
check_tz <- function(timezone) {
  if (!is.null(timezone) && timezone == "") return("")
  if (is.null(timezone) || !timezone %in% c("UTC", "Europe/Berlin")) return("UTC")
  timezone
}
failing_call <- function(expr) {
  tryCatch({ expr; NULL }, error = function(e) deparse(conditionCall(e))[[1]])
}
stopifnot(identical(check_kind(), "warning"))
stopifnot(identical(check_tz(NULL), "UTC"))
stopifnot(identical(check_tz("UTC"), "UTC"))
stopifnot(startsWith(failing_call(check_kind(c("stop", "ignore"))), "if (kind == \"stop\")"))
stopifnot(identical(failing_call(check_tz(c("UTC", "UTC"))), "!is.null(timezone) && timezone == \"\""))
# An empty value is NA as a head of `&&`/`||`, so only an evaluated tail is
# pinned to length one.
stopifnot(identical(if (character(0) == "" && FALSE) 1 else 2, 2))
stopifnot(identical(if (character(0) == "" || TRUE) 1 else 2, 1))
