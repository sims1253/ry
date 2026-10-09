# oracle: must-pass
# A dot belongs to the module basename; box loads foo.bar.r, not foo.r.
box::use(./box_module/foo.bar[answer])
stopifnot(identical(answer() + 1L, 2L))
