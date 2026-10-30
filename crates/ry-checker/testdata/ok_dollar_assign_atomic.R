# no-diag
# `$<-` on an atomic vector warns and coerces the LHS to a list; R
# raises the "$ operator is invalid for atomic vectors" error only when
# reading. The checker must not report RY061 for the write (#840).
x <- "not a list"
x$col <- 1L
