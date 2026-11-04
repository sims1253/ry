# oracle: must-flag-only RY000
# R rejects a function whose formals repeat a name ("repeated formal
# argument 'x'"); tree-sitter accepts it (#841).
f <- function(x, x) x
