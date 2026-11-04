# oracle: must-flag-only RY000
# R's lexer rejects an unknown string escape ("'\q' is an unrecognized
# escape in character string"); tree-sitter accepts it (#841).
x <- '\q'
