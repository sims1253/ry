# oracle: must-pass
# A qualified caller-frame writer adds a real legacy export at load time.
box::use(./box_module/dynamic_assign[foo])
stopifnot(identical(foo(), 1L))
