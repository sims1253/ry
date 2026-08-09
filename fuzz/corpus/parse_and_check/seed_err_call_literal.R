# expect: RY070, RY070, RY070
# Calling a literal value errors in R ("attempt to apply non-function").
a <- 42()
b <- TRUE()
c <- NULL()
