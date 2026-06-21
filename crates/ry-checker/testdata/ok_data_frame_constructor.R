# no-diag
# `data.frame(x = c(1L, 2L, 3L), y = c("a","b","c"))` attaches class
# "data.frame" and builds a column schema with coerced column lengths;
# `df$x` resolves cleanly to integer<3>.
df <- data.frame(x = c(1L, 2L, 3L), y = c("a", "b", "c"))
xv <- df$x
