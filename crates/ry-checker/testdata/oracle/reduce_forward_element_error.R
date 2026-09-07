# oracle: must-flag
# The second operand is an input element, even with no initializer.
Reduce(function(a, b) b | TRUE, c('a', 'b'))
