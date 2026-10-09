# oracle: must-pass
# A selected dplyr filter has data-mask semantics without attaching dplyr.
box::use(dplyr[filter])
d <- data.frame(mpg = c(21, 22.8))
chosen <- filter(d, mpg > 21)
stopifnot(nrow(chosen) == 1L, chosen$mpg == 22.8)
box::use(dp = dplyr)
chosen_alias <- dp$filter(d, mpg > 21)
stopifnot(identical(chosen_alias, chosen))
local({
  box::use(dplyr[other = filter])
  stopifnot(identical(other(d, mpg > 21), chosen))
  stopifnot(!exists("dplyr", inherits = FALSE))
})
local({
  box::use(dplyr[other = filter, ...])
  stopifnot(identical(other(d, mpg > 21), chosen))
  stopifnot(!exists("filter", inherits = FALSE))
})
