# expect: RY060, RY060, RY060
# Two genuine data-mask true positives: dplyr supplies `.data` at
# runtime, so a column outside mtcars' known schema is a real miss in
# filter() and mutate(). These must fire identically before and after
# the source-less mask fix (#383). The final `with()` line pins checker
# schema behavior only -- base R's `with()` provides no `.data` pronoun,
# so it is retention coverage, not an R-truth claim.
library(dplyr)
a <- filter(mtcars, .data$nonexistent > 1)
b <- mutate(mtcars, z = .data$nonexistent)
c <- with(mtcars, .data$nonexistent)
