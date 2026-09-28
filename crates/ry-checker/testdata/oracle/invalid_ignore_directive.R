# oracle: must-warn RY112
# oracle-claim: RY112
# R ignores comments; a misspelled ry code has no runtime effect.
stopifnot(1L + 1L == 2L) # ry: ignore[RX040]
