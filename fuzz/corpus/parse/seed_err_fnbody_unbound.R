# expect: RY010
# A reference to an undefined variable inside a named function body
# must fire RY010.
f <- function() { y <- undefined_variable_xyz }
