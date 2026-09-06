# oracle: must-pass
`+.left` <- function(e1, e2) "left"
`+.right` <- function(e1, e2) "right"
chooseOpsMethod.left <- function(x, y, mx, my, cl, reverse) TRUE
x <- structure(data.frame(a = 1), class = c("left", "data.frame"))
y <- structure(2, class = "right")
# A data.frame subclass can return an atomic value through operator dispatch.
stopifnot(identical(x + 1, "left"), identical(x + x, "left"))
stopifnot(identical(x + y, "left"), identical(y + x, "left"))
# The base method still preserves an ordinary data frame's columns.
d <- data.frame(a = 1, b = 2)
stopifnot(identical(d + 1, data.frame(a = 2, b = 3)))
