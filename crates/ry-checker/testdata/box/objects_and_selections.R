# expect: RY118
# A plain import binds the module object, a selection binds the name only.
box::use(../oracle/box_module/hello)
box::use(../oracle/box_module/hello[foo])
hello$foo()
foo()
box::use(../oracle/box_module/hello[missing])
