# oracle: must-pass
x <- structure(1L,class='left')
y <- structure(2L,class='right')
`+.left` <- function(e1,e2) 'left'
`+.right` <- function(e1,e2) 1L
chooseOpsMethod.left <- function(...) TRUE
chooseOpsMethod.right <- function(...) TRUE
`:=` <- function(...) { chooseOpsMethod.left <<- function(...) FALSE }
ignored := 1L
out <- x + y
stopifnot(identical(out, 1L))
out + 1L
