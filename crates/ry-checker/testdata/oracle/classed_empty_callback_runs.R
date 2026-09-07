# oracle: must-flag
x <- structure(character(), class = "ry_callback_nonempty")
as.list.ry_callback_nonempty <- function(x, ...) list(1L)
Filter(function(v) 1L + "bad", x)
