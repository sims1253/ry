# oracle: must-flag
# The data mask changes name resolution, not expression checking: a
# literal type error inside a masked expression still errors in R, so
# the mask must not swallow expression diagnostics (#369's adjacent
# control).
flights <- data.frame(month = 1:12, dep_time = 401:412)
with(flights, "a" + 1L)
