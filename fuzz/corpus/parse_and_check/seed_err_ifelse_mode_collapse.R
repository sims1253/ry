# expect: RY106, RY106, RY106, RY106
# ifelse() seeds its result from the test vector itself and only
# overwrites the positions the test selects, so the result mode is
# logical whenever the test is zero-length or entirely NA, even when both
# branches agree on another mode. Each call below is the minimal form of
# tidyverse/hms#231, where `as.character(hms())` returned `logical(0)`
# instead of `character(0)`.
#
# The audit shape, pre-fix hms R/hms.R:215: the test is an open-world
# parameter (maybe empty), and both branches are character.
format_hms <- function(x) {
  ifelse(is.na(x), NA_character_, paste0("00:", x))
}
# Zero-length test: nothing is overwritten, so the result stays logical(0).
a <- ifelse(logical(0), NA_character_, "a")
# All-NA test: no position is selected, so the logical NA survives.
b <- ifelse(NA, "a", "b")
# Integer branches lose their mode the same way.
c <- ifelse(logical(0), 1L, 2L)
