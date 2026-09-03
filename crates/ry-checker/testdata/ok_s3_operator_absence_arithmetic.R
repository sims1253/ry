# no-diag
# No operator or `Ops` method exists for `plain`, and the project defines
# none either: the operator falls back to the base-type arithmetic rules
# silently, exactly as R's internal primitive does.
x <- structure(c(1, 2), class = "plain")
y <- x + 1
