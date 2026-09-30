# oracle: must-pass
# Explicit export can re-export a selected name from another local module.
box::use(./box_module/reexport)
stopifnot(identical(reexport$foo(), 1L))
