# expect: RY090, RY090, RY090, RY090, RY090, RY090
# A module's imported paste0 hides base's; formals survive unknown returns.
box::use(m = ../oracle/box_module/imported_return[foo])
foo() + 1L
m$foo() + 1L
foo(wrong = 1L)
m$foo(wrong = 1L)
box::use(scoped = ../oracle/box_module/function_imported_return[local_foo = foo])
local_foo() + 1L
scoped$foo() + 1L
local_foo(wrong = 1L)
scoped$foo(wrong = 1L)
box::use(../oracle/box_module/imported_return[...])
foo() + 1L
foo(wrong = 1L)
box::use(../oracle/box_module/function_imported_return[...])
foo() + 1L
foo(wrong = 1L)
