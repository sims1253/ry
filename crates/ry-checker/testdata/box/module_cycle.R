# no-diag
# An import cycle terminates. A module that reaches one stays opaque: what
# each side binds depends on which module box loads first.
box::use(./modules/cycle/a[from_a, from_b, missing])
from_a + 1L
from_b + 1L
