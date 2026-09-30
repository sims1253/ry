# oracle: must-warn RY118
# oracle-claim: RY118
# A complete explicit export set contains foo, so box rejects missing.
failure <- tryCatch(
  box::use(./box_module/hello[missing]),
  error = function(err) err
)
stopifnot(inherits(failure, "error"))
