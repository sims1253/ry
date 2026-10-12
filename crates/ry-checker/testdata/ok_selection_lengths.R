# no-diag
# Selections keep the first argument's mode and class but re-count its
# elements: none of these results has the input's length, so no condition
# or sum below may claim it does. R runs every line cleanly.
x <- c(3, 1, 2)
if (head(x, 1) > 0) print("head")
if (tail(x, 1) > 0) print("tail")
if (unique(c(5, 5)) > 1) print("unique")
if (setdiff(c(1, 2, 3), c(2, 3)) == 1) print("setdiff")
if (union(c(1, 1), 1) == 1) print("union")
if (na.omit(c(1, NA)) == 1) print("na.omit")
if (subset(x, x > 2) > 0) print("subset")
if (window(ts(1:10), 1, 1) > 0) print("window")
y <- head(1:10, 3) + c(1, 2, 3)

# A data frame keeps its columns while the selection changes its rows.
df <- data.frame(a = 1:3, b = c("x", "y", "z"))
z <- head(df, 2)$a + c(10, 20)
w <- subset(df, a > 1)$a + c(10, 20)

# A selection may drop the out-of-range element, so no cast is proven.
b <- as.integer(head(c(1, 1e10), 1))
d <- as.integer(setdiff(c(1e10, 1), 1e10))
