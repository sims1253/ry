# oracle: must-flag-only RY000
# R rejects a string escape that encodes NUL ("nul character not
# allowed"); tree-sitter accepts it (#841).
x <- "a\0b"
