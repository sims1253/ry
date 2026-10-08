# oracle: must-pass
# Export tags span blank lines and ordinary comments and permit ##' prefixes.
box::use(regions = ./box_module/export_regions[blank, ordinary, hashes, attached])
stopifnot(identical(blank, 1L), identical(ordinary, 2L), identical(hashes, 3L))
stopifnot(identical(attached(), 1L))
stopifnot(identical(sort(names(regions)), c("attached", "blank", "hashes", "ordinary")))
