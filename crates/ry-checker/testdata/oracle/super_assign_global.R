# oracle: must-pass
# <<- assigns into the enclosing (global) scope from inside a function.
g <- 0
f <- function() { g <<- 10 }
f()
print(g)
