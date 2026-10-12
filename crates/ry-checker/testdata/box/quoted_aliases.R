# no-diag
# Quoted aliases bind their decoded names in tagged and legacy modules.
box::use(../oracle/box_module/quoted_tagged[obj, renamed])
box::use(../oracle/box_module/quoted_legacy[obj])
box::use(../oracle/box_module/quoted_strings[obj, renamed])
obj$foo() + renamed()
