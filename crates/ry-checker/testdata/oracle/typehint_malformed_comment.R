# oracle: must-pass
# oracle-claim: RY117
# A malformed adopted annotation is still an R comment, not an R parse error.
src <- "f <- function(x) {\n #| x\n x\n}\n"
stopifnot(!inherits(try(parse(text = src), silent = TRUE), "try-error"))
