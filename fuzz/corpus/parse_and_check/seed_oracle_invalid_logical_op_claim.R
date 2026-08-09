# oracle: must-pass
# oracle-claim: RY031
# Character values cannot participate in R's logical binary operators.
error <- tryCatch(eval(parse(text = '"x" & TRUE')), error = identity)
stopifnot(inherits(error, "error"))
