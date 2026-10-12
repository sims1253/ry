# no-diag
box::use(dplyr[other = filter])
box::use(dplyr)
d <- data.frame(mpg = c(21, 22.8))
other(d, mpg > 21)
dplyr$filter(d, mpg > 21)
dplyr$`filter`(d, mpg > 21)
dplyr$"filt\u0065r"(d, mpg > 21)
