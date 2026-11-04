# oracle: must-pass
# `ry dump-facts` classes for unary `-` and `!` with no `Ops` method,
# checked against GNU R by the ry-cli facts oracle (#890). The primitive
# keeps every attribute when the result keeps the operand's storage mode,
# empty input included, and drops them after a coercion.
# oracle-facts: negated negated_int negated_pair negated_empty inverted inverted_raw coerced inverted_number plain
negated <- -structure(1, class = "foo")
negated_int <- -structure(1L, class = "foo")
negated_pair <- -structure(c(1, 2), class = c("a", "b"))
negated_empty <- -structure(numeric(0), class = "foo")
inverted <- !structure(c(TRUE, NA), class = "foo")
inverted_raw <- !structure(as.raw(1), class = "foo")
coerced <- -structure(TRUE, class = "foo")
inverted_number <- !structure(1, class = "foo")
plain <- -1
