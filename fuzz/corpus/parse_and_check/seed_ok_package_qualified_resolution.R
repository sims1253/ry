# no-diag
# pkg::-qualified calls resolve against the named package's typeshed
# WITHOUT requiring library(). Covers dplyr, purrr, mirai, and the
# multi-package bayes typeshed (brms/posterior/loo/bayesplot/cmdstanr).
#
xs <- purrr::map(1:3, function(x) x * 2)
dbls <- purrr::map_dbl(1:3, function(x) x + 0.5)
m <- mirai::mirai(sqrt(2))
d <- dplyr::tibble(a = 1:3)
fit <- brms::brm(y ~ 1, data = d)
draws <- posterior::as_draws_df(fit)
lo <- loo::loo(list())
plt <- bayesplot::ppc_dens_overlay(1, matrix(NA, 1, 1))
# stats:: (merged into base) resolves under the stripped name.
r <- stats::rnorm(10)
