# no-diag
box::use(../oracle/box_module/package_reexport)
box::use(../oracle/box_module/package_reexport[filter])
d <- data.frame(mpg = c(21, 22.8))
filter(d, mpg > 21)
package_reexport$filter(d, mpg > 21)
