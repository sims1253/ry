# oracle: must-pass
# The final callable replaces both an earlier literal and imported metadata.
box::use(./box_module/expression_reassigned[local = foo])
box::use(./box_module/imported_reassigned[imported = foo])
stopifnot(identical(local() + 1L, 2L), identical(imported() + 1L, 2L))
