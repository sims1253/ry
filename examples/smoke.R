# Example file: each line demonstrates one inference case.

# OK: int + double -> double
x <- 1L + 2.0

# OK: scalar comparison -> length-1 logical
cond <- x > 0

# DIAG RY040: char + int is invalid
y <- "a" + 1L

# OK: c() of mixed coerces upward
nums <- c(1L, 2L, 3.0)

# DIAG RY001: `if` on character
if ("x") print(nums)

# DIAG RY002: `if` on length-2 logical
if (c(TRUE, FALSE)) print(1)

# OK: function definition
f <- function(a = 1L, b = 2.0) {
  a + b
}

# DIAG RY010: unbound identifier
z <- undefined_thing
