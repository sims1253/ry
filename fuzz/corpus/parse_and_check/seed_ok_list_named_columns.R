# no-diag
# `list(a = 1L, b = "x")` builds a column schema from the named args,
# so `l$a` resolves to integer<1> and `l$b` to character<1>.
l <- list(a = 1L, b = "x")
v <- l$a
