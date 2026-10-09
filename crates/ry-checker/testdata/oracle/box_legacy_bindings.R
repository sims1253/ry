# oracle: must-pass
# Bare module/package imports create own bindings; nested braces execute.
box::use(./box_module/legacy_package[dplyr])
box::use(./box_module/legacy_local[hello])
box::use(./box_module/legacy_braced[x])
stopifnot(identical(hello$foo(), 1L), identical(x + 1L, 2L))
stopifnot(is.function(dplyr$filter))
