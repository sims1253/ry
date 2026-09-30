# oracle: must-pass
# Explicit exports override roxygen tags, empty calls block legacy fallback,
# multiple calls combine, and legacy modules exclude leading-dot names.
box::use(override = ./box_module/override)
box::use(empty = ./box_module/empty)
box::use(union = ./box_module/union)
box::use(legacy = ./box_module/legacy)
stopifnot(identical(names(override), "bar"))
stopifnot(length(names(empty)) == 0L)
stopifnot(identical(sort(names(union)), c("bar", "foo")))
stopifnot(identical(names(legacy), "foo"))
local({
  box::use(./box_module/override[renamed = bar, ...])
  stopifnot(identical(renamed(), 2L), !exists("bar", inherits = FALSE))
})
