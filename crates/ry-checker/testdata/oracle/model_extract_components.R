# oracle: must-pass
mf <- stats::model.frame(y ~ x, data.frame(y = 1:3, x = 4:6))
expected <- stats::model.response(mf)
stopifnot(identical(stats::model.extract(mf, response), expected))
stopifnot(identical(stats::model.extract(component = response, frame = mf), expected))
stopifnot(identical(stats::model.extract(mf, comp = response), expected))
forced <- FALSE
stopifnot(identical(stats::model.extract({ forced <- TRUE; mf }, response), expected), forced)
