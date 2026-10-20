# oracle: must-flag-only RY092
# `$` on the bare formal demands a non-atomic receiver (#646).
nm <- function(df) df$value * 2
nm(1:3)
