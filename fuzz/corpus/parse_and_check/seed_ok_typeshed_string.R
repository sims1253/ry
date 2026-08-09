# no-diag
# String functions from the expanded typeshed: `toupper`, `tolower`,
# `gsub`, `substr`, `trimws`, `paste`, `paste0` all return character.
# Using `paste0` to concatenate is well-typed.
x <- c("hello", "world")
upper <- toupper(x)
lower <- tolower(x)
trimmed <- trimws(x)
clean <- gsub("o", "0", x)
sub <- substr(x, 1, 3)
result <- paste0(upper, lower)
