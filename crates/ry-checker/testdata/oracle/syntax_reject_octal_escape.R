# oracle: must-flag-only RY000
# An octal escape holds one byte, so R rejects `\400` ("\400 exceeds
# maximum allowed octal value \377"); tree-sitter accepts it (#841).
x <- "\400"
