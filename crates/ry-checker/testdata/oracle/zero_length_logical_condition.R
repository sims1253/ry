# oracle: must-warn RY001
# A zero-length logical is not a missing value: `if` and `while` both reject
# it before choosing a branch.
condition <- logical(0)
if_error <- tryCatch(if (condition) TRUE else FALSE, error = identity)
while_error <- tryCatch(while (condition) break, error = identity)
stopifnot(identical(conditionMessage(if_error), "argument is of length zero"))
stopifnot(identical(conditionMessage(while_error), "argument is of length zero"))
