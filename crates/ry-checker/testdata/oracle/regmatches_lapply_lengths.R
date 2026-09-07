# oracle: must-pass
x <- c("aa", "z")
matches <- regmatches(x, gregexpr("a", x))
result <- lapply(matches, function(z) {
  if (length(z) == 0L) return(z)
  toupper(z)
})
stopifnot(identical(result, list(c("A", "A"), character())))

# Inversion returns a list even with regexpr's vector match data.
inverse <- regmatches("ab", regexpr("a", "ab"), invert = TRUE)
stopifnot(identical(inverse, list(c("", "b"))))
stopifnot(identical(regmatches("ab", regexpr("a", "ab")), "a"))
