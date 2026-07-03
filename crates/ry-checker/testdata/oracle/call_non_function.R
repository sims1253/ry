# oracle: must-flag
# Calling a non-function errors in R: could not find function "x".
# KNOWN LIMITATION: ry currently does not flag `42()` (calling a
# numeric literal). The oracle surfaces this gap. Tracked as a future
# rule (RY050 "calling a non-function" enhancement).
x <- 42()
