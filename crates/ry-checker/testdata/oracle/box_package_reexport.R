# oracle: must-pass
# Explicitly re-exported package functions keep the original package's
# data-mask contract through the module object and a selected binding.
box::use(./box_module/package_reexport)
box::use(./box_module/package_reexport[filter])
d <- data.frame(mpg = c(21, 22.8))
chosen <- filter(d, mpg > 21)
stopifnot(identical(chosen, package_reexport$filter(d, mpg > 21)))
