# expect: RY040
# lapply result accessed via [[i]]: the callback return type is
# inferred (integer), so result[[1]] resolves to integer. Using the
# result arithmetically with a character fires RY040.
result <- lapply(1L:3L, function(i) i * 2L)
x <- result[[1]]
bad <- x + "hello"
