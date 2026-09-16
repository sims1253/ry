# expect: RY001
# The type update lands at the definition's position in the sequential
# walk (issue #374): a read that precedes the `<<-`-writing closure's
# definition cannot have seen the write at runtime either, so the stale
# NULL type still fires here.
x <- NULL
while (x) break
writer <- function() x <<- TRUE
