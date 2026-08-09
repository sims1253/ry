# no-diag
# `factor(...)` infers class "factor", and `print.factor` is in the base R
# typeshed's S3 method table, so `print(f)` dispatches cleanly.
f <- factor(c("a", "b"))
print(f)
