# no-diag
# ifelse with yes_or_no mode inference: the result mode is the join of
# the yes/no args. `ifelse(x > 0, "pos", "neg")` returns character.
x <- c(-1, 0, 1)
result <- ifelse(x > 0, "pos", "neg")
upper <- toupper(result)
