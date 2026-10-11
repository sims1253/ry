# oracle: must-pass
# When a module exists as both `.r` and `.R`, box loads the lowercase file.
# The pair is created at run time: a case-insensitive checkout cannot hold it.
dir <- tempfile()
dir.create(file.path(dir, "mod"), recursive = TRUE)
writeLines("answer <- function() 1L", file.path(dir, "mod", "preferred.r"))
writeLines("answer <- function() 'wrong'", file.path(dir, "mod", "preferred.R"))
if (length(list.files(file.path(dir, "mod"))) == 2L) {
  options(box.path = dir)
  box::use(mod/preferred[answer])
  stopifnot(identical(answer(), 1L))
}
