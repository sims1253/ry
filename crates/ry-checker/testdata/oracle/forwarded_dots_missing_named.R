# oracle: must-warn RY091
# An exact named empty argument still occupies its required formal.
target <- function(x, y) x + y
forward <- function(...) target(x = , ...)
error <- tryCatch(forward(y = 2L), error = function(condition) condition)
stopifnot(inherits(error, "error"),
          grepl('"x" is missing', conditionMessage(error), fixed = TRUE))
