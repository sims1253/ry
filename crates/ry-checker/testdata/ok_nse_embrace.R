# no-diag
library(dplyr)

my_summarise <- function(df, group_var) {
  dplyr::summarise(df, mean = mean({{ group_var }}))
}
