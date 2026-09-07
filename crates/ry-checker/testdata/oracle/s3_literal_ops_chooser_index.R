# oracle: must-pass
x <- structure(1L,class='left')
y <- structure(2L,class='right')
`*.left` <- function(e1,e2) 'left'
`*.right` <- function(e1,e2) 1L
chooseOpsMethod.left <- function(...) TRUE
chooseOpsMethod.right <- function(...) TRUE
`[` <- function(value,index) { chooseOpsMethod.left <<- function(...) FALSE; index }
out <- x[(x * y) + 1L]
stopifnot(identical(out, 2L))
