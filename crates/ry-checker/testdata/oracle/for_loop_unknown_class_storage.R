# oracle: must-flag
# R refuses class "factor" on anything but integer storage, so `for` over
# a character vector binds its strings whatever the unknown class is, and
# `v + 1` errors (#861).
loop_plus_one <- function(cls) {
  s <- structure(c("a", "b"), class = cls)
  for (v in s) print(v + 1)
}
loop_plus_one("tagged")
