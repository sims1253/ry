# expect: RY060
# A leading negative selector starts from every column: `-x, y` keeps g.
wide <- data.frame(x = 1L, y = 2L, g = 3L)
kept <- dplyr::select(wide, -x, y)
kept$g + 1L
kept$x
