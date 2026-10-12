# expect: RY040, RY040
box::use(./modules/text_function[f1 = foo])
box::use(./modules/text_reexport[f2 = foo])
f1() + 1L
f2() + 1L
