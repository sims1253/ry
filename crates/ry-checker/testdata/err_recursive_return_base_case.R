# expect: RY040, RY040, RY040
# A recursive call contributes no value until its own return is known, so
# the base case decides the return type instead of an unknown seed.
f <- function(n) if (n == 0L) "oops" else f(n - 1L)
f(1L) + 1L

g <- function(n) {
  if (n == 0L) return("done")
  h(n - 1L)
}
h <- function(n) g(n)
h(2L) + 1L

count <- function(n) if (n == 0L) 0L else count(n - 1L) + 1L
count(3L) + "a"
