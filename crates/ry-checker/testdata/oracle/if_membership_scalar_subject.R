# oracle: must-pass
# `x %in% table` has one element per element of `x`, so a passed `if`
# membership test proves `x` scalar: a vector or NULL errors at the first
# test and never reaches the later `is.null(x) || pred(x)` guard (#702).
check_kind <- function(kind = "warning") {
  if (!kind %in% c("ignore", "warning", "stop")) stop("bad kind")
  if (is.null(kind) || kind == "ignore") return(NULL)
  kind
}
failing_call <- function(expr) {
  tryCatch({ expr; NULL }, error = function(e) deparse(conditionCall(e))[[1]])
}
stopifnot(identical(check_kind(), "warning"))
stopifnot(is.null(check_kind("ignore")))
first_test <- "if (!kind %in% c(\"ignore\", \"warning\", \"stop\")) stop(\"bad kind\")"
stopifnot(identical(failing_call(check_kind(c("stop", "ignore"))), first_test))
stopifnot(identical(failing_call(check_kind(NULL)), first_test))
# A project `mtfrm` method decides what `match()` compares for a classed
# subject, so it can return fewer elements than the subject has.
mtfrm.kind_label <- function(x) "warning"
labelled <- structure(c("stop", "ignore"), class = "kind_label")
stopifnot(identical(length(labelled %in% "warning"), 1L))
