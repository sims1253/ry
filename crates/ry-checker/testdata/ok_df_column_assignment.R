# no-diag
# `$<-` and `[[<-` update a known data-frame schema, so later reads of
# assigned columns should resolve.
fit_grid <- data.frame(model = "gaussian", stringsAsFactors = FALSE)
fit_grid$formula <- list(y ~ x)
fit_grid[["family"]] <- list("gaussian")

formula_value <- fit_grid$formula
family_value <- fit_grid[["family"]]
