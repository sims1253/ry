# no-diag
count_arm <- function() {
  df <- data.frame(TRTP = factor(c("Placebo", "Drug")))
  arm <- "Placebo"
  sum(df$TRTP == arm)
}
