# oracle: must-flag
x <- 1L
while (TRUE) {
    break
    x <- function() 1L
}
x$field
