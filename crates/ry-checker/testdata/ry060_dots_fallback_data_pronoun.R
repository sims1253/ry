# expect: RY060, RY060
# Retention pins for data-leading signatures whose data formal has no
# eval entry of its own (only `...` is data-masked): the first argument
# IS the data frame, so schema-driven pronoun diagnostics must survive
# the own-binding guard from #383 (these 31 signatures -- dplyr
# transmute/count, tidyr expand/complete, survival tmerge, rlist
# list.* -- previously lost their RY060 true positives through the
# `...` eval fallback).
library(dplyr)
library(tidyr)
a <- dplyr::transmute(mtcars, z = .data$nonexistent)
b <- tidyr::expand(mtcars, .data$nonexistent)
