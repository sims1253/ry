# oracle: must-pass
# An ordinary `<<-` can run an active binding's setter, which replaces a
# different name: `mutate2()` rebinds `f2`, not just `g2`. ry's outward-write
# summary sees only `g2`: a known RY114 false positive (the `g <<-` case of
# `a_known_invoked_helper_invalidates_only_its_outward_writes`).
makeActiveBinding("g2", function(v) { if (!missing(v)) f2 <<- function(x) x; 1 }, environment())
f2 <- function(x) { stopifnot(identical(class(x), "integer")); x }
mutate2 <- function() g2 <<- function(x) x
before <- tryCatch(f2("before"), error = function(e) "error")
mutate2()
stopifnot(identical(before, "error"), identical(f2("after"), "after"))
