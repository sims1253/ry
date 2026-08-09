# no-diag
d <- data.frame(a = 1:10, b = 11:20)
m <- d[, 1]
if (m[3] >= 0.5) print("hi")
if (d[, "a"][2] > 0) print("named")
x <- c(10, 20, 30)
if (x[2] > 1) print("scalar")
if (length(logical(0)) == 1) print("length is scalar")
