# oracle: must-warn RY051
# oracle-claim: RY051
# Primitive fallback bypasses Ops.factor even for a factor-tagged operand.
x <- structure(1L, class = "factor")
y <- structure(2L, class = "right")
`+.factor` <- function(e1, e2) 1L
`+.right` <- function(e1, e2) "right"
chooseOpsMethod.factor <- function(...) FALSE
chooseOpsMethod.right <- function(...) FALSE
out <- x + y
stopifnot(identical(unclass(out), 3L), identical(class(out), "factor"))
messages <- names(warnings())
stopifnot(length(messages) == 1L,
          any(grepl("Incompatible methods", messages, fixed = TRUE)),
          !any(grepl("not meaningful for factors", messages, fixed = TRUE)))
