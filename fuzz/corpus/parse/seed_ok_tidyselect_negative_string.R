# no-diag
reshape_for_display <- function() {
  df <- data.frame(Treatment = c("A", "B"), N = c(10, 12), mean = c(1.2, 1.4))
  long <- tidyr::pivot_longer(
    df,
    cols = -"Treatment",
    names_to = "Statistic",
    values_to = "Value"
  )
  dplyr::select(long, -"Value")
}
