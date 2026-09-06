# oracle: must-pass
`+.left` <- function(e1, e2) "left"
`+.right` <- function(e1, e2) "right"
x <- structure(1, class = "left")
y <- structure(2, class = "right")
# Distinct methods fall back to numeric storage under default selection.
stopifnot(unclass(suppressWarnings(x + y)) + 1 == 4)
stopifnot(unclass(suppressWarnings(y + x)) + 1 == 4)
# The primitive result retains the left operand's class. Without unclass(),
# the following addition dispatches again through that class's method.
stopifnot(identical(suppressWarnings(x + y) + 1, "left"))
stopifnot(identical(suppressWarnings(y + x) + 1, "right"))
# One-sided and identical-method dispatch still choose the method.
stopifnot(identical(x + 1, "left"), identical(x + x, "left"))
# Custom selection can prevent primitive fallback entirely.
chooseOpsMethod.left <- function(x, y, mx, my, cl, reverse) TRUE
stopifnot(identical(x + y, "left"))
