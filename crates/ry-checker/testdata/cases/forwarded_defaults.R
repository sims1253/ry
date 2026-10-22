## case: a guard around the forwarding call drops the forwarded default
# no-diag
# xgboost's xgb.train forwards `save_period` only under this guard, so
# the callee never receives the caller's NULL default (#719).
cb_save <- function(save_period = 0) {
  if (save_period < 0) stop("'save_period' cannot be negative")
  save_period
}
train <- function(data, save_period = NULL) {
  if (!is.null(save_period)) {
    cb_save(save_period)
  }
  data
}
fit <- function(data) train(data)

## case: a type-predicate guard drops the forwarded default
# no-diag
parse_file <- function(path = "a.R") if (nchar(path) > 0) path
read_all <- function(path = NULL) if (is.character(path)) parse_file(path)
run <- function() read_all()

## case: an early-return guard drops the forwarded default
# no-diag
cb_save <- function(save_period = 0) if (save_period < 0) stop("negative")
train <- function(save_period = NULL) {
  if (is.null(save_period)) return(NULL)
  cb_save(save_period)
}
fit <- function() train()

## case: a short-circuit guard drops the forwarded default
# no-diag
cb_save <- function(save_period = 0) if (save_period < 0) FALSE else TRUE
train <- function(save_period = NULL) !is.null(save_period) && cb_save(save_period)
fit <- function() train()

## case: an unguarded forwarding call still forwards the NULL default
# expect: RY001
cb_save <- function(save_period = 0) if (save_period < 0) stop("negative")
train <- function(save_period = NULL) cb_save(save_period)
fit <- function() train()
