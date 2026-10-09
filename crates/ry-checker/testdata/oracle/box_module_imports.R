# oracle: must-pass
# box's selective import binds foo but not the module object; an explicit
# alias binds both. These are runtime assertions for the static fixtures.
box::use(./box_module/hello)
stopifnot(identical(hello$foo(), 1L))
local({
  box::use(./box_module/hello[foo])
  stopifnot(identical(foo(), 1L), !exists("hello", inherits = FALSE))
})
local({
  box::use(alias = ./box_module/hello[renamed = foo])
  stopifnot(identical(alias$foo(), 1L), identical(renamed(), 1L))
  stopifnot(!exists("foo", inherits = FALSE))
})
