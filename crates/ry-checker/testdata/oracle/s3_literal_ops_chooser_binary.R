# oracle: must-pass
x <- structure(1L,class='left')
y <- structure(2L,class='right')
`*.left` <- function(e1,e2) 'left'
`*.right` <- function(e1,e2) 1L
chooseOpsMethod.left <- function(...) TRUE
chooseOpsMethod.right <- function(...) TRUE
`^` <- function(value,power) { chooseOpsMethod.left <<- function(...) FALSE; value }
out <- ((x * y) + 1L) ^ 2L
stopifnot(identical(out, 2L))
