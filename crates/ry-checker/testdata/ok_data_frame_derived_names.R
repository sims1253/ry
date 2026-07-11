# no-diag
# data.frame derives simple positional names and normalizes quoted labels.
y <- 1:3
K <- 3L
d <- data.frame(y, K, "energy__" = 1:3)
d$y
d$K
d$energy__
