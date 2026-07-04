# no-diag
# A magrittr pipe chain through dplyr verbs. `mtcars %>% filter(cyl == 4)`
# desugars to `filter(mtcars, cyl == 4)`; the result (a data frame) then
# pipes into `select(mpg, hp)`. Both verbs resolve column references
# against the augmented scope built from the piped data frame, so `cyl`,
# `mpg`, and `hp` all resolve and no RY010 is emitted.
library(magrittr)
library(dplyr)
result <- mtcars %>% filter(cyl == 4) %>% select(mpg, hp)
