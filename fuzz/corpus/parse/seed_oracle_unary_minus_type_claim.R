# oracle: must-pass
# oracle-claim: RY020
# Unary minus rejects a character operand. The text is parsed dynamically so
# this fixture tests R's premise independently of the checker trigger probe.
error <- tryCatch(eval(parse(text = '-"hello"')), error = identity)
stopifnot(inherits(error, "error"))
