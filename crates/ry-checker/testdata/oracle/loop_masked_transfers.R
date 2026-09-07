# oracle: must-pass
`break` <- function() NULL
`next` <- function() NULL
x <- 0L
for (i in 1:2) {
    break
    next
    x <- i
}
stopifnot(identical(x, 2L))
