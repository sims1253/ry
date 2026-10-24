# oracle: must-pass
# R's dispatcher binds .Generic, .Class, .Method, and .Group (possibly "")
# in every S3 method frame (#348): a helper registered under another name,
# Logic and matrixOps operator methods, and UseMethod methods alike.
# Packages register such helpers in evalqOnLoad(), which runs only at
# namespace load, so this script registers them at top level.
dispatched <- function(generic, method) {
  frame <- parent.frame()
  stopifnot(
    identical(get(".Generic", frame), generic),
    identical(get(".Method", frame)[[1L]], method),
    identical(get(".Class", frame), "widget"),
    is.character(get(".Group", frame))
  )
}
compare_widget <- function(e1, e2) {
  stopifnot(identical(.Generic, "<"), identical(.Class, "widget"), .Group == "")
  NextMethod(.Generic)
}
for (op in c("<", ">")) {
  registerS3method(op, "widget", compare_widget)
}
format_widget <- function(x, ...) {
  stopifnot(.Generic == "format", .Method == "format.widget", .Class == "widget", .Group == "")
  "widget"
}
.S3method("format", "widget", format_widget)
`!.widget` <- function(x) {
  dispatched("!", "!.widget")
  .Group
}
`&.widget` <- function(e1, e2) {
  dispatched("&", "&.widget")
  .Group
}
`|.widget` <- function(e1, e2) {
  dispatched("|", "|.widget")
  .Group
}
`%*%.widget` <- function(x, y) {
  dispatched("%*%", "%*%.widget")
  .Group
}
print.widget <- function(x, ...) {
  dispatched("print", "print.widget")
  invisible(c(.Generic, .Class, .Method, .Group))
}
x <- structure(1, class = "widget")
stopifnot(identical(getS3method("<", "widget"), compare_widget))
stopifnot(isFALSE(as.vector(x < x)), format(x) == "widget")
!x
x & x
x | x
x %*% x
print(x)
