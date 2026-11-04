# oracle: must-pass
# `ry dump-facts` claims for atomic vectors, checked against typeof(),
# length(), and oldClass() by the ry-cli facts oracle (#858).
# oracle-facts: int dbl chr lgl raw_byte null
# oracle-facts: seq_ints rep_doubles pasted
int <- 1L
dbl <- c(1.5, 2, 3)
chr <- c("a", "b")
lgl <- TRUE
raw_byte <- as.raw(1)
null <- NULL
seq_ints <- seq_len(5)
rep_doubles <- rep(1, 4)
pasted <- paste0("a", 1:3)
