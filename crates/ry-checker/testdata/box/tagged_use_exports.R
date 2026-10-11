# expect: RY118
# A tagged use exports its object and attached aliases.
box::use(../oracle/box_module/tagged_reexport[imp, renamed, local_value, missing])
box::use(../oracle/box_module/tagged_attached[renamed])
box::use(../oracle/box_module/tagged_wildcard[foo, unknown])
