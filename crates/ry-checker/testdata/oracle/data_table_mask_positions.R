# oracle: must-pass
# data.table evaluates i, j, `by`, and `.SDcols` in a mask over the
# receiver's columns. `month` is a column here even though a same-named
# function is defined in this file: the column shadows it exactly where
# the mask applies, and `:=` left-hand names denote columns to create or
# replace rather than references (#369).
month <- function(x) format(x)
dt <- data.table::as.data.table(data.frame(month = 1:12, day = 1:12, dep_time = 401:412))
june <- dt[month == 6L]
stopifnot(nrow(june) == 1L, identical(june$dep_time, 406L))
dt[, dep_time := NA_integer_]
dt[, june := month == 6L]
stopifnot(all(is.na(dt$dep_time)), all(dt$june == (dt$month == 6L)))
agg <- dt[, .(mean_month = mean(month)), by = day]
stopifnot(nrow(agg) == 12L, identical(agg$mean_month[1L], 1))
scaled <- dt[, lapply(.SD, max), .SDcols = c("month", "day")]
stopifnot(identical(scaled$month, 12L), identical(scaled$day, 12L))
