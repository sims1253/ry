# oracle: must-warn RY101
args <- list(font = "monospace")
stopifnot(!identical(args["font"], "monospace"))
stopifnot(identical(args[["font"]], "monospace"))
