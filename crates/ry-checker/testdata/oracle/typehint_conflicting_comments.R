# oracle: must-pass
# oracle-claim: RY116
# R ignores both comments. Selecting either as a static contract when the
# authored annotations disagree would be a ry decision, not R behavior.
src <- "f <- function(x) {\n #| x integer\n #| x character\n x\n}\n"
stopifnot(length(gregexpr("#\\| x", src)[[1L]]) == 2L)
stopifnot(!inherits(try(parse(text = src), silent = TRUE), "try-error"))
