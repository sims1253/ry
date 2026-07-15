# expect: RY050
# `print.default` means print's S3 dispatch always has a valid fallback, so
# it must not be used as a missing-method fixture. A package-local generic
# without a default still emits RY050 when it has no matching class method.
Summary.other <- function(...) 1L
x <- structure(list(), class = "undefined")
Summary(x)
