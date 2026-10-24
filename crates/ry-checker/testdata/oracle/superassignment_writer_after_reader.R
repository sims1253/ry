# oracle: must-pass
# A closure body runs when it is called, not where it is defined (issue
# #706): a `<<-` writer defined after the reader closure in the source can
# still run before the reader does. ry gives every `<<-` target an unknown
# type inside closure bodies, whatever the definition order, and stays
# silent here.
spin_control <- function() {
  pressed <- NULL
  rotate <- function() {
    if (pressed) "rotating"
  }
  on_press <- function() {
    pressed <<- TRUE
  }
  list(rotate = rotate, on_press = on_press)
}
control <- spin_control()
control$on_press()
stopifnot(identical(control$rotate(), "rotating"))

helper_late <- NULL
api <- local({
  run <- function(x) helper_late(x)
  helper_late <<- function(x) x * 2
  run
})
stopifnot(identical(api(2), 4))

state <- NULL
run_state <- function() if (state) "on"
setup <- function() state <<- TRUE
setup()
stopifnot(identical(run_state(), "on"))

block_writer <- function() {
  flag <- NULL
  read_flag <- function() if (flag) "set"
  local({ flag <<- TRUE })
  read_flag
}
stopifnot(identical(block_writer()(), "set"))
