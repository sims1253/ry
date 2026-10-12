# expect: RY080
# library(tidyverse) attaches purrr, so map_dbl's callback contract applies
# exactly as after library(purrr).
library(tidyverse)
xs <- map_dbl(1:3, function(x) paste("n", x))
