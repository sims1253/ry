# oracle: must-pass
x <- c(5L, 6L)
x[is.na(x)] <- 1e10
stopifnot(typeof(x) == "double")
stopifnot(identical(as.integer(x), c(5L, 6L)))
x <- integer(0)
x[is.na(x)] <- 1e10
stopifnot(identical(as.integer(x), integer(0)))
x <- c(5, NA_real_)
x[is.na(x)] <- c(0, 1e10)
stopifnot(identical(as.integer(x), c(5L, 0L)))
x <- as.integer(structure(5, class = "Date"))
x[is.na(x)] <- 1e10
stopifnot(identical(as.integer(x), 5L))
x <- (as.integer(1e10) # handled
)
x[is.na(x)] <- 0L
stopifnot(identical(x, 0L))
