# oracle: must-warn RY118
# Module `$` uses exact names: foo exists, fo does not partially match.
box::use(./box_module/hello)
failure <- tryCatch(hello$fo, error = function(err) err)
stopifnot(inherits(failure, "error"))
