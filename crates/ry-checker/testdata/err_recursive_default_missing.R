# expect: RY109
# missing() inspects whether a promise was supplied without forcing its
# self-referential default, so this runs. Writing a default that names the
# formal is still meaningless -- when missing, the promise is the default --
# so RY109 warns.
f <- function(x = x) missing(x)
f()
