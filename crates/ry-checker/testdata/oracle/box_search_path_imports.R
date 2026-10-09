# oracle: must-pass
# Search-path modules use box.path while their static bindings stay opaque in ry.
local({
  script <- sub("^--file=", "", grep("^--file=", commandArgs(), value = TRUE))
  previous <- options(box.path = dirname(script))
  on.exit(options(previous))
  box::use("module" = box_module/hello[attached = foo])
  object <- module
  callback <- attached
  stopifnot(identical(object$foo(), 1L), identical(callback(), 1L))
  box::use(box_module/hello[...])
  wildcard <- foo
  stopifnot(identical(wildcard(), 1L))
})
