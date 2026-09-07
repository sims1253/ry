# oracle: must-pass
confint.ry_contract <- function(object, parm, level = 0.95, ...) {
  list(interval = c(-1, 1), estimate = NA_real_, level = level)
}
model <- structure(list(), class = 'ry_contract')
ci <- stats::confint(model, level = .9)
stopifnot(is.list(ci), identical(ci$interval, c(-1, 1)), is.na(ci$estimate))
fit <- lm(y ~ x + z, data = data.frame(y = c(1, 3, 2, 5), x = 1:4, z = 2*(1:4)))
ci <- stats::confint(fit)
stopifnot(is.matrix(ci), is.double(ci), anyNA(ci))
