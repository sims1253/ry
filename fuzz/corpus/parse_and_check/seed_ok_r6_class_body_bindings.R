# no-diag
Thing <- R6Class(
  "Thing",
  public = list(
    get = function() self$value,
    reveal = function() private$secret,
    parent = function() super$clone()
  ),
  private = list(secret = 1L)
)

Other <- new_class(list(run = function() self$value + private$value + super$value))
