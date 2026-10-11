# no-diag
# Explicit exports override tags; several export calls combine.
box::use(../oracle/box_module/override[bar])
box::use(../oracle/box_module/union[foo, bar])
