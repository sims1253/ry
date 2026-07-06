# no-diag
extract_survfit_quantiles <- function() {
  fit <- survival::survfit(survival::Surv(AVAL, 1 - CNSR) ~ TRTP, data = data.frame())
  qfit <- quantile(fit)
  med_tte <- qfit$quantile[, "50"]
  med_lower <- qfit$lower[, "50"]
  med_upper <- qfit$upper[, "50"]
  list(med_tte, med_lower, med_upper)
}
