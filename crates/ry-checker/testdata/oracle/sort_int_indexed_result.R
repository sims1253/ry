# oracle: must-pass
x <- sort.int(c(2, 1), method = "quick", index.return = TRUE)
stopifnot(identical(x$ix, c(2L, 1L)), identical(x$x, c(1, 2)))
stopifnot(identical(sort.int(c(2L, 1L)), c(1L, 2L)))
y <- sort(c(2, 1), method = "quick", index.return = TRUE)
stopifnot(identical(y$ix, c(2L, 1L)), identical(y$x, c(1, 2)))
stopifnot(identical(sort(c(2L, 1L)), c(1L, 2L)))
