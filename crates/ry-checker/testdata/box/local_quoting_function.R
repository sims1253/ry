# expect: RY090
# The module's quoting formal, not dplyr::filter, describes the call.
box::use(../oracle/box_module/local_filter[filter])
d <- data.frame(x = 1L)
filter(d, missing_column)
filter(d, wrong = 1L)
