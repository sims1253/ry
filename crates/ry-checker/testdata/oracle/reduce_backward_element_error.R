# oracle: must-flag
# A right fold passes the input element as its first operand.
Reduce(function(a, b) a | TRUE, c('a', 'b'), right = TRUE)
