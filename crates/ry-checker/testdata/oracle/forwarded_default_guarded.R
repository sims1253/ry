# oracle: must-pass
# A guard on the forwarded formal keeps the caller's NULL default away
# from the callee (xgboost's xgb.train / xgb.cb.save.model shape).
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
parse_file <- function(path = "a.R") if (nchar(path) > 0) path
read_all <- function(path = NULL) if (is.character(path)) parse_file(path)
run <- function() read_all()
early <- function(save_period = NULL) {
  if (is.null(save_period)) return(NULL)
  cb_save(save_period)
}
check <- function(save_period = NULL) !is.null(save_period) && cb_save(save_period) >= 0
stopifnot(identical(fit(1), 1), is.null(run()), is.null(early()), !check())
