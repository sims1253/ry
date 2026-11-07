# oracle: must-flag-only RY000
# R's lexer rejects a numeric literal whose exponent has no digits
# ("unexpected input"); tree-sitter accepts `1e` as a float (#841).
x <- 1e
