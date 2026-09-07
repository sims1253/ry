# oracle: must-warn RY051
x <- structure(1L, class = "left")
y <- structure(2L, class = "right")
`+.left` <- function(e1, e2) 1L
`+.right` <- function(e1, e2) 2L
chooseOpsMethod.left <- function(...) FALSE
chooseOpsMethod.right <- function(...) FALSE
out <- x + y
stopifnot(identical(unclass(out), 3L), identical(class(out), "left"))
stopifnot(any(grepl("Incompatible methods", names(warnings()), fixed = TRUE)))
