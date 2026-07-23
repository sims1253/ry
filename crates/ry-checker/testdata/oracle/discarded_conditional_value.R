# oracle: must-warn RY099
nudge <- function(z) {
  if (z == 0) z + 0.001
  z
}
stopifnot(nudge(0) == 0)
