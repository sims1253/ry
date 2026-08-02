# oracle: known-gap self-referential formal defaults recurse when forced
outer <- 1
recursive_default <- function(outer = outer) outer
recursive_default()
