# oracle: must-pass
p23 <- expand.grid(0:2, 0:2)
value <- 2^p23[, 1] * 3^p23[, 2]
stopifnot(identical(value, c(1, 2, 4, 3, 6, 12, 9, 18, 36)))
