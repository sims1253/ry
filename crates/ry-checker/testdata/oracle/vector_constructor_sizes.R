# oracle: must-pass
stopifnot(length(integer(0)) == 0L, length(numeric(10)) == 10L)
stopifnot(length(raw(len = 3)) == 3L, length(logical(2.9)) == 2L)
stopifnot(length(vector(length = 2, "integer")) == 2L)
stopifnot(length(vector()) == 0L, length(numeric(-0.2)) == 0L)
