# expect: RY040, RY040
# Two modules re-export one shared module.
box::use(left = ./modules/diamond/left, right = ./modules/diamond/right)
left$foo() + 1L
right$foo() + 1L
