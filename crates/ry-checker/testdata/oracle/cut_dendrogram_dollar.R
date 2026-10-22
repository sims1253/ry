# oracle: must-pass
# cut() is an S3 generic; on a dendrogram it dispatches to
# cut.dendrogram, which returns list(upper =, lower =), so `$` works
# (#711). The vendored base stub declares cut in s3_generics.
dend <- stats::as.dendrogram(stats::hclust(stats::dist(1:5)))
parts <- cut(dend, h = 2)
stopifnot(is.list(parts), identical(names(parts), c("upper", "lower")))
stopifnot(inherits(cut(dend, h = 2)$upper, "dendrogram"))
stopifnot(length(cut(dend, h = 2)$lower) == 2)
