# no-diag
# Statistical functions: `median`, `var`, `sd`, `quantile`, `IQR`,
# `mad` all return double. Using results arithmetically with another
# double is well-typed.
x <- c(1.0, 2.0, 3.0, 4.0, 5.0)
a <- median(x) + 1.0
b <- sd(x) + 1.0
c <- var(x) + 1.0
d <- IQR(x) + 1.0
e <- mad(x) + 1.0
