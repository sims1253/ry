# oracle: must-pass
# The exported callable is the replacement, not the earlier literal.
box::use(./box_module/reassigned[foo])
stopifnot(identical(foo() + 1L, 2L))
