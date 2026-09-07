# oracle: must-pass
f <- function(flag) {
  scale <- 1L
  trial <- list()
  while (TRUE) {
    trial$score <- 1L
    if (flag) break
    trial <- list(alpha = 2)
  }
  if (flag) scale <- 1 + abs(trial$score) else scale <- abs(trial$score)
  if (scale > 0) TRUE
}
stopifnot(f(TRUE))
