# no-diag
# Iterating over a list yields the UNWRAPPED element type, not list<1>.
# So arithmetic inside the lapply/sapply callback must not fire RY040.
out <- lapply(list(1, 2, 3), function(x) x * 2)
out2 <- sapply(list(1, 2, 3), function(x) x * 2)
total <- 0
for (el in list(1, 2, 3)) {
  total <- total + el
}
