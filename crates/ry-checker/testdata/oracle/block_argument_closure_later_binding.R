# oracle: must-warn RY010
# A closure looks up its free names when it runs, in the frame where it was
# defined. A braced call argument (`suppressWarnings()`, a `switch()` arm,
# `local()`) may therefore bind them after the definition (issue #690). Only
# `not_bound`, which no frame binds, is reported.
fit_in_block <- function(x) {
  suppressWarnings({
    loss <- function(s) sum((x - mu)^2) / s
    mu <- mean(x)
    loss(2)
  })
}
stopifnot(identical(fit_in_block(c(1, 3)), 1))
fit_in_switch <- function(method, x) {
  switch(method, reml = {
    loss <- function(s) sum((x - mu)^2) / s
    mu <- mean(x)
    loss(2)
  })
}
stopifnot(identical(fit_in_switch("reml", c(1, 3)), 1))
scaled_total <- local({
  scaled_total <- function(xs) sum(vapply(xs, get_x, numeric(1))) * SCALE
  get_x <- function(z) z[["x"]]
  SCALE <- 2
  scaled_total
})
stopifnot(identical(scaled_total(list(list(x = 1), list(x = 2))), 6))
unbound <- tryCatch(suppressWarnings({
  read <- function() not_bound
  read()
}), error = conditionMessage)
stopifnot(grepl("not_bound", unbound, fixed = TRUE))
