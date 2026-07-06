# no-diag
summarise_then_use_columns <- function() {
  df <- data.frame(AVAL = c(1, 2, 3), flag = c(TRUE, FALSE, TRUE))
  summary <- dplyr::summarise(
    df,
    n = sum(!is.na(AVAL)),
    Mean = dplyr::if_else(n == 0, NA_real_, mean(AVAL, na.rm = TRUE)),
    responders = sum(flag),
    N = dplyr::n(),
    .groups = "drop"
  )
  summary <- dplyr::mutate(
    summary,
    rate = ifelse(N > 0, responders / N, NA_real_)
  )
  paste0(summary$responders, "/", summary$N, " ", summary$rate)
}
