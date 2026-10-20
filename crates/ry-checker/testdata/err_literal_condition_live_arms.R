# expect: RY090 RY090 RY090 RY099
# The arm a literal `if` condition does take is live and is still checked;
# only the arm it never takes is silent (issue #703).
f <- function(x, y = 1) x + y

if (TRUE) f(1, z = 2)
if (FALSE) NULL else f(1, w = 2)
k <- if (TRUE) f(1, q = 2) else 3

g <- function(x) {
  if (TRUE) paste0(x, "a")
  if (FALSE) paste0(x, "b")
  x
}

# The dead twins stay quiet in the same file.
if (FALSE) f(1, z = 2)
if (TRUE) NULL else f(1, w = 2)
kk <- if (FALSE) f(1, q = 2) else 3
