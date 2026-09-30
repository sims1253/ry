# oracle: must-pass
# When both spellings exist, lowercase .r wins.
box::use(./box_module/preferred[answer])
stopifnot(identical(answer() + 1L, 2L))
