# no-diag
# `vapply` with FUN.VALUE template: the result type is dictated by
# `FUN.VALUE` (here, `numeric(1)` = double<1>). The callback is walked
# for type inference. Using the result arithmetically is well-typed.
v <- vapply(c(1, 2, 3), function(x) x * 2, numeric(1))
y <- v + 1.0
