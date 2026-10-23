# oracle: must-pass
# `x[[N]]` counts every element, named or not.
x <- list(a = "s", list(b = 2))
stopifnot(identical(x[[2]]$b + 1, 3))
y <- list(0, list(b = 1))
stopifnot(identical(y[[2]]$b, 1))

# `z$a <- NULL` drops `a` and shifts the later elements down.
z <- list(a = 1, "s", list(b = 2))
z$a <- NULL
stopifnot(identical(z[[2]]$b, 2))
