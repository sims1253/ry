# no-diag
# `%<>%` (magrittr assignment pipe) rebinds the LHS identifier to the
# result of the pipe: `x %<>% sqrt()` is `x <- x %>% sqrt()`. After the
# pipe, `x` has the result type (double), so the downstream `y <- x + 1`
# resolves cleanly. Guards a past bug where `%<>%` shared the result
# type with `%>%` but did NOT rebind, so `x` kept its pre-pipe type.
x <- 4
x %<>% sqrt()
y <- x + 1
