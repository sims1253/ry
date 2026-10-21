# oracle: must-warn RY001
# A positive type predicate is FALSE for NULL, so a guarded block on a
# NULL-default parameter only runs for a caller-supplied value: the
# default's zero length must not reach it (#695). A value that really is
# zero-length still errors, and ry still warns there.
add_columns <- function(.data, .before = NULL) {
  if (is.character(.before)) {
    if (!(.before %in% names(.data))) .before <- NULL
  }
  .before
}
draw <- function(caption = NULL) {
  if (is.logical(caption)) {
    if (!caption) caption <- NULL
  }
  caption
}
weigh <- function(weights_attr = NULL) {
  if (inherits(weights_attr, "character")) {
    if (weights_attr == "w") 1 else 2
  }
}
stopifnot(!is.character(NULL), !is.logical(NULL), !inherits(NULL, "character"))
stopifnot(is.null(add_columns(data.frame(a = 1))), is.null(draw()), is.null(weigh()))
stopifnot(identical(add_columns(data.frame(a = 1), "a"), "a"))
stopifnot(is.null(add_columns(data.frame(a = 1), "b")))
stopifnot(isTRUE(draw(TRUE)), is.null(draw(FALSE)))
stopifnot(identical(weigh(structure("w", class = "character")), 1))

really_empty <- function() {
  x <- character(0)
  if (is.character(x)) {
    if (x == "a") 1 else 2
  }
}
error <- tryCatch(really_empty(), error = identity)
stopifnot(inherits(error, "error"))
stopifnot(identical(conditionMessage(error), "argument is of length zero"))
