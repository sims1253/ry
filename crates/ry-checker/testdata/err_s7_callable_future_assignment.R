# expect: RY070
# A later callable assignment cannot change the concrete value at an earlier
# call site.
x <- 1L
x()
x <- S7::new_class("x")
