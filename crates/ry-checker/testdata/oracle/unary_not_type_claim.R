# oracle: must-pass
# oracle-claim: RY021
# Logical negation rejects a character operand.
error <- tryCatch(eval(parse(text = '!"hello"')), error = identity)
stopifnot(inherits(error, "error"))
