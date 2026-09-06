# oracle: must-pass
# Argument matching rejects these calls before forcing the recursive default.
wrong_name <- function(x = x) base::identity(unused = x)
extra <- function(x = x) base::identity(1L, unused = x)
for (f in list(wrong_name, extra)) {
  result <- tryCatch(f(), error = function(e) e)
  stopifnot(inherits(result, "error"), grepl("unused argument", conditionMessage(result)))
}
