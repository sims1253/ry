# oracle: must-pass
# A complex `<<-` target whose root an intervening formal binds still
# reaches the definition scope when the root is reference-typed: R's
# complex superassignment fetches the environment through the formal
# and mutates it in place (verified: the file-level `env$key` reads
# back TRUE after `outer(env)()`, so the loop runs once). ry reserves
# the intervening-formal prune for plain-name rebinding and records the
# complex root (issue #374), staying silent on the condition.
env <- new.env()
env$key <- NULL
outer <- function(env) {
  inner <- function() env$key <<- TRUE
  inner
}
outer(env)()
stopifnot(identical(env$key, TRUE))
count <- 0L
while (env$key) {
  count <- count + 1L
  env$key <- FALSE
}
stopifnot(identical(count, 1L))
