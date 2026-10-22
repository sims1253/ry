# oracle: must-pass
# `$` on a list partially matches: a unique prefix returns that field,
# an exact name wins over longer names, and an ambiguous prefix is NULL
# (#721). `[[` matches exactly, and `$<-` writes a new field.
res <- list(nvalid = 3L, total = 6)
stopifnot(identical(res$total / res$nv, 2))
stopifnot(identical(list(nv = 1, nvalid = 2)$nv, 1))
stopifnot(is.null(list(nvalid = 3L, nvx = 1)$nv))
stopifnot(is.null(res[["nv"]]))
res$nv <- 5
stopifnot(identical(names(res), c("nvalid", "total", "nv")))
