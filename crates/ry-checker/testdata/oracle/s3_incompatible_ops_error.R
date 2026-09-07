# oracle: must-flag
x <- structure("text", class = "left")
y <- structure(2L, class = "right")
`+.left` <- function(e1, e2) 1L
`+.right` <- function(e1, e2) "right"
chooseOpsMethod.left <- function(...) FALSE
chooseOpsMethod.right <- function(...) FALSE
out <- x + y
