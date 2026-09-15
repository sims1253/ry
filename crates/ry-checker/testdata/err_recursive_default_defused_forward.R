# expect: RY109
# A user helper whose formal is captured with substitute() does not force the
# recursive default passed to it, so this runs when the argument is missing.
# The default itself can never evaluate, so RY109 warns without a forcing
# proof; RY098 stays quiet because defusing is not forcing.
capture <- function(z) substitute(z)
f <- function(x = x) capture(x)
f()
