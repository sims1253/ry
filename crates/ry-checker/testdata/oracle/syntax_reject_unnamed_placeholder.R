# oracle: must-flag-only RY000
# The `|>` placeholder must be a named argument ("pipe placeholder can
# only be used as a named argument"); tree-sitter accepts `f(_)` (#841).
x <- 1
y <- x |> c(_)
