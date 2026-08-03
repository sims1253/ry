# no-diag
margin <- S7::new_class("margin", parent = S7::class_double)
titleGrob <- function(margin = NULL) {
  if (is.null(margin)) margin <- margin(0, 0, 0, 0)
  margin
}

verb <- S7::new_generic("verb", "x")
verb(NULL)

old_class <- S7::new_S3_class("old_class")
old_class(NULL)
