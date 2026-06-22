# expect: RY070
# Calling a non-function value: R errors at runtime.
x <- 42
y <- x(10)
