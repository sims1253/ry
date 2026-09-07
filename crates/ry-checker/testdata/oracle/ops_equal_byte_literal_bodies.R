# oracle: must-pass
x <- structure(1L, class = "left")
y <- structure(2L, class = "right")
`+.left` <- function(e1, e2) "\xff"
`+.right` <- function(e1, e2) "\377"
chooseOpsMethod.left <- function(...) FALSE
chooseOpsMethod.right <- function(...) FALSE
out <- x + y
stopifnot(identical(out, "\xff"), length(warnings()) == 0L)
