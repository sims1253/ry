# expect: RY040
# `text` is inferred to return character (its body is a literal string),
# so using its result as an arithmetic operand is a type error.
text <- function() {
  "hello"
}
y <- text() + 1L
