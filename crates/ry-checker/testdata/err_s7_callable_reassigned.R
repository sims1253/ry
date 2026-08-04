# expect: RY070
# A later ordinary assignment replaces an earlier callable S7 object.
x <- S7::new_class("x")
x <- 1L
x()
