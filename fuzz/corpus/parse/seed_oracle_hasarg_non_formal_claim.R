# oracle: must-pass
# oracle-claim: RY096
# Without `...`, hasArg() reports FALSE for a name that is not a formal.
library(methods)
no_dots <- function(x) hasArg(threshold)
stopifnot(identical(no_dots(1L), FALSE))
