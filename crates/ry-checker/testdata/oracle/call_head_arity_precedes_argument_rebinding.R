# oracle: must-flag
f <- function(x, y) { x; y; 1L }
f({ f <- function(x) { x; 1L }; 1L })
