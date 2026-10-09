# oracle: must-pass
# ../ resolves from the containing module, not the process working directory.
box::use(./box_module/nested/caller)
stopifnot(identical(caller$run(), 1L))
