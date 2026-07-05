# expect: RY040
# union[list, function] + 1 fires RY040: every member of the union
# errors against `+ 1` (an op on a union errors only when ALL members
# error). This pins the all-invalid case that distinguishes a
# real arithmetic bug from a quiet some-member-ok union.
p <- TRUE
x <- if (p) list(1) else function() { 1 }
y <- x + 1
