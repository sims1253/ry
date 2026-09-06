# oracle: must-warn RY098
# oracle-claim: RY098
and_default <- function(x = x) TRUE && x
or_default <- function(x = x) FALSE || x
nested <- function(x = x) TRUE && (FALSE || x)
qualified <- function(x = x) base::force(TRUE && x)
late <- function(x = value) {
  FALSE || x
  value <- TRUE
}
for (f in list(and_default, or_default, nested, qualified)) {
  result <- tryCatch(f(), error = function(e) e)
  stopifnot(inherits(result, "error"), grepl("promise already under evaluation", conditionMessage(result)))
}
result <- tryCatch(late(), error = function(e) e)
stopifnot(inherits(result, "error"), grepl("value", conditionMessage(result)))
