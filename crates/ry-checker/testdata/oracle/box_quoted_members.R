# oracle: must-pass
# Quoted box members identify the same exported values and callables.
box::use(m = ./box_module/hello, alias = ./box_module/member_aliases)
stopifnot(identical(m$foo, m$`foo`), identical(m$foo, m$"foo"))
stopifnot(identical(m$foo(), 1L), identical(m$`foo`(), 1L))
stopifnot(identical(m$"foo"(), 1L), identical(m$`\x66oo`(), 1L))
stopifnot(identical(m$"\u0066oo"(), 1L), identical(alias$`my-fn`(), 1L))
stopifnot(identical(alias$"my-\u0066n"(), 1L))
