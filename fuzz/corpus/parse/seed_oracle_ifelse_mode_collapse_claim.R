# oracle: must-pass
# oracle-claim: RY106
# ifelse() seeds its result from the test vector itself and only
# overwrites the positions the test selects, so the result mode is
# logical whenever the test is zero-length or entirely NA, even when
# both branches are character. A mixed test does coerce back.
stopifnot(identical(typeof(ifelse(logical(0), NA_character_, "a")), "logical"))
stopifnot(identical(ifelse(logical(0), NA_character_, "a"), logical(0)))
stopifnot(identical(typeof(ifelse(NA, "a", "b")), "logical"))
stopifnot(identical(ifelse(NA, "a", "b"), NA))
stopifnot(identical(typeof(rep(NA, 2)), "logical"))
stopifnot(identical(typeof(ifelse(rep(NA, 2), 1L, 2L)), "logical"))
stopifnot(identical(typeof(ifelse(c(TRUE, NA), "a", "b")), "character"))
