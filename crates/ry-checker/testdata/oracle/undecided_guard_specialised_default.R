# oracle: must-pass
# A guard ry cannot decide that reads a defaulted parameter may be the very
# test the omitted default fails. Each guarded arm below is dead under the
# default, so the default's type must not report inside it (#710).
is_tensor <- function(x) inherits(x, "tensor")
numel <- function(dims = 2) if (is_tensor(dims)) dims$numel()
lambda_of <- function(model = "ZZZ", lambda = NULL) {
  if (inherits(model, "ets") && is.null(lambda)) model$lambda
}
pt_of <- function(width = "\\linewidth") {
  if (sum(c("pt", "lw") %in% colnames(width)) > 0) width$pt
}
values_of <- function(sel = FALSE) if ("values" %in% names(sel)) sel$values
stopifnot(is.null(numel()), is.null(lambda_of()), is.null(pt_of()), is.null(values_of()))
stopifnot(identical(values_of(list(values = 1)), 1))
