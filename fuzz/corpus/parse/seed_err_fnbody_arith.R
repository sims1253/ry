# expect: RY040
# Function bodies are walked for diagnostics: arithmetic between a
# character and an integer inside a named function body fires RY040.
f <- function() { x <- "hello" + 1 }
