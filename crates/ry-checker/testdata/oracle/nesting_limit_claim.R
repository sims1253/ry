# oracle: must-pass
# oracle-claim: RY129
# R's parser accepts nesting far deeper than ry's limit of 128 levels: its
# bison stack allows thousands of unbracketed levels. So RY129 marks valid R
# that ry did not check, not a syntax error (#887).
deep <- paste0(strrep("-", 512), "1")
stopifnot(identical(eval(parse(text = deep)), 1))
assigned <- paste0(strrep("a <- ", 512), "2")
stopifnot(identical(eval(parse(text = assigned)), 2))
# Bracket nesting is the exception: R stops at 49 open brackets.
nested <- function(n) paste0(strrep("(", n), "1", strrep(")", n))
stopifnot(identical(eval(parse(text = nested(49))), 1))
overflow <- tryCatch(parse(text = nested(50)), error = conditionMessage)
stopifnot(grepl("contextstack overflow", overflow))
