# oracle: known-gap switch EXPR must have length one
choose_alert <- function(type = c("success", "info", "warning", "danger")) {
  switch(type, success = 1L, info = 2L, warning = 3L, danger = 4L)
}
choose_alert()
