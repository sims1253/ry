# oracle: must-pass
x <- "old"
while (TRUE) {
    repeat { break }
    unused <- function() { break }
    lapply(integer(), function(value) { break })
    x <- 1L
    break
    x <- function() 1L
}
stopifnot(identical(x, 1L))
for (i in 1:2) {
    x <- 2L
    next
    x <- function() 1L
}
stopifnot(identical(x, 2L))
