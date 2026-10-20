# expect: RY040, RY040, RY040, RY040
# Symbols and calls built by `quote()` carry no class, so primitive
# arithmetic rejects them ("non-numeric argument to binary operator").
q <- quote(a + b)
q * 2
s <- quote(a)
s + 1
make <- function() quote(a + b)
make() * 2
v <- if (runif(1) > 0.5) quote(a) else "x"
v + 1
