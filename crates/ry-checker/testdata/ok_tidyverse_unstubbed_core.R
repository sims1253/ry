# no-diag
# library(tidyverse) also attaches forcats and lubridate, which have no
# stubs, so their exports stay resolvable as unknown names.
library(tidyverse)
f <- fct_rev(factor(c("a", "b")))
d <- ymd("2020-01-01")
