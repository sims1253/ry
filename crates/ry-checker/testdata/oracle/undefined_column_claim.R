# oracle: must-pass
# oracle-claim: RY060
# `$` on a data frame whose schema lacks the requested column returns NULL.
# Dynamic evaluation keeps this R premise separate from the trigger probe.
value <- eval(parse(text = "datasets::mtcars$ry_oracle_missing_column"))
stopifnot(is.null(value))
