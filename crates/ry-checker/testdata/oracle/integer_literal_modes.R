# oracle: must-pass
# R keeps an L suffix only when the value fits its integer storage.
decimal <- 1e5L
hexadecimal <- 0x10L
largest <- 2147483647L
wide <- 2147483648L
rounded <- 9007199254740993L
fractional <- 1.5L
wide_hexadecimal <- 0x80000000L
stopifnot(identical(decimal, 100000L), identical(hexadecimal, 16L),
          typeof(largest) == "integer", typeof(wide) == "double",
          identical(rounded, 9007199254740992), identical(fractional, 1.5),
          identical(wide_hexadecimal, 2147483648))
