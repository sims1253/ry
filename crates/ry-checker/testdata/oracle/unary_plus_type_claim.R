# oracle: must-pass
# oracle-claim: RY020
# Unary plus rejects a character operand (#891). The text is parsed
# dynamically so this fixture tests R's premise independently of the checker
# trigger probe.
error <- tryCatch(eval(parse(text = '+"a"')), error = identity)
stopifnot(inherits(error, "error"))
