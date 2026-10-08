# oracle: must-pass
# The module's imported paste0 replaces base's character-returning function.
box::use(module = ./box_module/imported_return[foo])
box::use(./box_module/function_imported_return[local_foo = foo])
stopifnot(identical(foo() + 1L, 2L))
stopifnot(identical(module$foo() + 1L, 2L))
stopifnot(identical(local_foo() + 1L, 2L))
