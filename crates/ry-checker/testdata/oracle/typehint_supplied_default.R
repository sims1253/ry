# oracle: must-pass
# oracle-claim: RY114
# The audited provider walks match.call() actuals; omitted defaults are absent.
was_supplied <- function(x = "wrong") "x" %in% names(as.list(match.call()))
stopifnot(!was_supplied())
stopifnot(was_supplied(x = "wrong"))
stopifnot(!identical(class("wrong"), "integer"))
