# no-diag
# The exported callable is not the indexed literal once replaced.
box::use(../oracle/box_module/reassigned[f1 = foo])
box::use(./modules/wrapped_alias[f2 = foo])
box::use(./modules/removed[f3 = foo])
box::use(./modules/direct_overwrite[f4 = foo])
box::use(./modules/conditional_overwrite[f5 = foo])
box::use(../oracle/box_module/expression_reassigned[f6 = foo])
box::use(./modules/runtime_overwrite[f7 = foo])
f1() + 1L
f2() + 1L
f3() + 1L
f4() + 1L
f5() + 1L
f6() + 1L
f7() + 1L
