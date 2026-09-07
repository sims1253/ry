# expect: RY040
x <- structure(1L, class = "left")
y <- structure(2L, class = "right")
`+.left` <- function(e1, e2) "left"
`+.right` <- function(e1, e2) 1L
chooseOpsMethod.left <- function(...) TRUE
selected <- x + y
selected + 1L
