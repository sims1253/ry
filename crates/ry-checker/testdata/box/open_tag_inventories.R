# no-diag
# Only static, exact export tags close an inventory.
box::use(../oracle/box_module/s3_legacy[bar])
box::use(../oracle/box_module/dynamic_tagged_export[bar])
box::use(./modules/computed_tagged_export[bar])
bar
