# no-diag
# Arithmetic is valid for these concrete integer inputs and predicate.
# Filter returns c(2L, 4L), and Find returns 2L.
even <- function(x) x %% 2 == 0
filtered <- Filter(even, c(1L, 2L, 3L, 4L))
found <- Find(even, c(1L, 2L, 3L))
y <- filtered + 1L
z <- found + 1L
