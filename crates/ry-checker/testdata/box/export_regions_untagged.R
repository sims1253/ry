# expect: RY118, RY118, RY118, RY118, RY118, RY118
# A tag inside a preceding expression's source annotates nothing after it.
box::use(../oracle/box_module/export_regions[hidden])
box::use(./modules/regions[before, after, absent])
box::use(./modules/tag_in_function[bar])
box::use(./modules/tag_same_line[bar])
