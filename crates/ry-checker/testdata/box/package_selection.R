# expect: RY010
# A selection binds the package function without attaching the package.
box::use(dplyr[filter])
d <- data.frame(mpg = c(21, 22.8))
filter(d, mpg > 21)
select(d, mpg)
