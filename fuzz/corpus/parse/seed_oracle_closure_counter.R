# oracle: must-pass
# Closure factory capturing state; R succeeds.
make_counter <- function() {
  n <- 0
  function() { n <<- n + 1; n }
}
ctr <- make_counter()
print(ctr())
print(ctr())
