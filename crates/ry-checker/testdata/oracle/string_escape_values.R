# oracle: must-pass
# Byte escapes encode bytes; Unicode escapes encode scalar values.
stopifnot(identical("\141", "a"), identical("\012", "\n"))
stopifnot(identical("\1412", "a2"), identical("\u{41}F", "AF"))
stopifnot(identical("\U{1F600}", "😀"))
stopifnot(identical("\xc3\xa9", "é"), identical("\303\251", "é"))
stopifnot(identical("\`", "`"))
stopifnot(identical("a\
b", "a\nb"))
x <- list(a = 1L)
stopifnot(identical(x[["\141"]], 1L), identical(x[["\u{61}"]], 1L))
