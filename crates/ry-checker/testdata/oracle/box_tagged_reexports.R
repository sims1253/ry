# oracle: must-pass
# A use-declaration export tag includes its object and selected aliases;
# a separate tagged assignment contributes another export.
box::use(./box_module/tagged_reexport[imp, renamed, local_value])
box::use(./box_module/tagged_attached[tagged_foo = renamed])
box::use(./box_module/tagged_wildcard[wild_foo = foo])
stopifnot(identical(imp$foo(), 1L), identical(renamed(), 1L))
stopifnot(identical(local_value, 1L), identical(tagged_foo(), 1L))
stopifnot(identical(wild_foo(), 1L))
