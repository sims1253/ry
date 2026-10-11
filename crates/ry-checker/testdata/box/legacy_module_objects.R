# expect: RY118, RY118
# Legacy exports include own module objects, not attached selections.
box::use(../oracle/box_module/legacy_package[dplyr, absent])
box::use(../oracle/box_module/legacy_local[hello])
box::use(./modules/legacy_attached[filter])
