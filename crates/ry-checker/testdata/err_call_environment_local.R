# expect: RY070
# R skips non-function bindings in call position, and no function named
# `registry` exists anywhere ("could not find function").
registry <- new.env()
registry()
