# oracle: must-pass
# merge() and read.csv() return data frames and gc() a double matrix,
# so matrix indexing yields columns that take arithmetic (#709). The
# vendored base stubs type these returns as a data frame, an opaque
# value, and a double matrix; they used to claim class-less lists.
a <- data.frame(k = 1:3, x = c(1, 2, 3))
b <- data.frame(k = 1:3, y = c(4, 5, 6))
m <- merge(a, b, by = 1)
stopifnot(identical(class(m), "data.frame"))
stopifnot(identical(m[, 1] - 1, c(0, 1, 2)))
stopifnot(is.data.frame(merge(a, a)[1] + 1))

r <- read.csv(text = "x,y\n1,2\n3,4")
stopifnot(identical(class(r), "data.frame"))
stopifnot(is.data.frame(100 * r[, c("x", "y")]))

g <- gc()
stopifnot(identical(class(g), c("matrix", "array")), is.double(g))
stopifnot(is.double(sum(g[, 1] * c(56, 8))))

stopifnot(inherits(try(list(1, 2)[, 1], silent = TRUE), "try-error"))
