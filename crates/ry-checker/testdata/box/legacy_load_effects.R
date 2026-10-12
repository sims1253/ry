# no-diag
# Load-time calls may create the selected names.
box::use(../oracle/box_module/dynamic_assign[foo])
box::use(./modules/sourced[foo])
box::use(./modules/sys_sourced[foo])
box::use(./modules/loaded[foo])
box::use(./modules/computed_load[foo])
foo
