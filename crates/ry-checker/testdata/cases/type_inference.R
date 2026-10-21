## case: character plus integer is a type error
# expect: RY040
"a" + 1L

## case: a rebinding before the call replaces the forwarded caller default
# no-diag
# A caller default reaches the callee's formal only while the caller
# never rebinds the name before the call. ggplot2's `compute_bins`
# rebinds `bins` this way before forwarding it.
callee <- function(x_range, bins = 30) {
if (bins == 1) 1 else 2
}
caller <- function(x, bins = NULL) {
bins <- allow_lambda(bins)
callee(range, bins)
}
z <- caller(c(0, 1))

## case: `bins` <- 1L drops the forwarded default
# no-diag
callee <- function(bins = 30) { if (bins == 1) 1L else 2L }
caller <- function(bins = NULL) { `bins` <- 1L; callee(`bins`) }
z <- caller()

## case: bins[1L] <- 1L drops the forwarded default
# no-diag
callee <- function(bins = 30) { if (bins == 1) 1L else 2L }
caller <- function(bins = NULL) { bins[1L] <- 1L; result <- callee(bins) }
z <- caller()

## case: bins[[1L]] <- 1L drops the forwarded default
# no-diag
callee <- function(bins = 30) { if (bins == 1) 1L else 2L }
caller <- function(bins = NULL) { bins[[1L]] <- 1L; print(callee(bins)) }
z <- caller()

## case: length(bins) <- 1L drops the forwarded default
# no-diag
callee <- function(bins = 30) { if (bins == 1) 1L else 2L }
caller <- function(bins = NULL) { length(bins) <- 1L; return(callee(bins)) }
z <- caller()

## case: assign('bins', 1L) drops the forwarded default
# expect: RY090
callee <- function(bins = 30) { if (bins == 1) 1L else 2L }
caller <- function(bins = NULL) { assign('bins', 1L); callee(`bins` = bins) }
z <- caller()

## case: base::assign(value = 1L, x = 'bins') drops the forwarded default
# no-diag
callee <- function(bins = 30) { if (bins == 1) 1L else 2L }
caller <- function(bins = NULL) { base::assign(value = 1L, x = 'bins'); callee(bins) }
z <- caller()

## case: delayedAssign('bins', 1L) drops the forwarded default
# no-diag
callee <- function(bins = 30) { if (bins == 1) 1L else 2L }
caller <- function(bins = NULL) { delayedAssign('bins', 1L); callee(bins) }
z <- caller()

## case: for (`bins` in ...) drops the forwarded default
# no-diag
callee <- function(bins = 30) { if (bins == 1) 1L else 2L }
caller <- function(bins = NULL) { for (`bins` in list(1L)) callee(bins) }
z <- caller()

## case: if ((`bins` <- 1L) > 0) drops the forwarded default
# no-diag
callee <- function(bins = 30) { if (bins == 1) 1L else 2L }
caller <- function(bins = NULL) { if ((`bins` <- 1L) > 0) print(callee(bins)) }
z <- caller()

## case: bins <<- (bins <- 1L) drops the forwarded default
# no-diag
callee <- function(bins = 30) { if (bins == 1) 1L else 2L }
caller <- function(bins = NULL) { bins <<- (bins <- 1L); print(callee(bins)) }
z <- caller()

## case: (bins <- 1L) ->> bins drops the forwarded default
# no-diag
callee <- function(bins = 30) { if (bins == 1) 1L else 2L }
caller <- function(bins = NULL) { (bins <- 1L) ->> bins; print(callee(bins)) }
z <- caller()

## case: bins <- (bins <<- 1L) drops the forwarded default
# no-diag
callee <- function(bins = 30) { if (bins == 1) 1L else 2L }
caller <- function(bins = NULL) { bins <- (bins <<- 1L); print(callee(bins)) }
z <- caller()

## case: bins <- (1L ->> bins) drops the forwarded default
# no-diag
callee <- function(bins = 30) { if (bins == 1) 1L else 2L }
caller <- function(bins = NULL) { bins <- (1L ->> bins); print(callee(bins)) }
z <- caller()

## case: 1L -> bins drops the forwarded default
# no-diag
callee <- function(bins = 30) { if (bins == 1) 1L else 2L }
caller <- function(bins = NULL) { 1L -> bins; print(callee(bins)) }
z <- caller()

## case: print({ bins <- 1L; ... }) drops the forwarded default
# no-diag
callee <- function(bins = 30) { if (bins == 1) 1L else 2L }
caller <- function(bins = NULL) { print({ bins <- 1L; callee(bins) }) }
z <- caller()

## case: backticked formal: `bins` <- 1L drops the forwarded default
# no-diag
callee <- function(`bins` = 30) { if (`bins` == 1) 1L else 2L }
caller <- function(bins = NULL) { `bins` <- 1L; callee(`bins`) }
z <- caller()

## case: backticked formal: bins[1L] <- 1L drops the forwarded default
# no-diag
callee <- function(`bins` = 30) { if (`bins` == 1) 1L else 2L }
caller <- function(bins = NULL) { bins[1L] <- 1L; result <- callee(bins) }
z <- caller()

## case: backticked formal: bins[[1L]] <- 1L drops the forwarded default
# no-diag
callee <- function(`bins` = 30) { if (`bins` == 1) 1L else 2L }
caller <- function(bins = NULL) { bins[[1L]] <- 1L; print(callee(bins)) }
z <- caller()

## case: backticked formal: length(bins) <- 1L drops the forwarded default
# no-diag
callee <- function(`bins` = 30) { if (`bins` == 1) 1L else 2L }
caller <- function(bins = NULL) { length(bins) <- 1L; return(callee(bins)) }
z <- caller()

## case: backticked formal: assign('bins', 1L) drops the forwarded default
# no-diag
callee <- function(`bins` = 30) { if (`bins` == 1) 1L else 2L }
caller <- function(bins = NULL) { assign('bins', 1L); callee(`bins` = bins) }
z <- caller()

## case: backticked formal: base::assign(value = 1L, x = 'bins') drops the forwarded default
# no-diag
callee <- function(`bins` = 30) { if (`bins` == 1) 1L else 2L }
caller <- function(bins = NULL) { base::assign(value = 1L, x = 'bins'); callee(bins) }
z <- caller()

## case: backticked formal: delayedAssign('bins', 1L) drops the forwarded default
# no-diag
callee <- function(`bins` = 30) { if (`bins` == 1) 1L else 2L }
caller <- function(bins = NULL) { delayedAssign('bins', 1L); callee(bins) }
z <- caller()

## case: backticked formal: for (`bins` in ...) drops the forwarded default
# no-diag
callee <- function(`bins` = 30) { if (`bins` == 1) 1L else 2L }
caller <- function(bins = NULL) { for (`bins` in list(1L)) callee(bins) }
z <- caller()

## case: backticked formal: if ((`bins` <- 1L) > 0) drops the forwarded default
# no-diag
callee <- function(`bins` = 30) { if (`bins` == 1) 1L else 2L }
caller <- function(bins = NULL) { if ((`bins` <- 1L) > 0) print(callee(bins)) }
z <- caller()

## case: backticked formal: bins <<- (bins <- 1L) drops the forwarded default
# no-diag
callee <- function(`bins` = 30) { if (`bins` == 1) 1L else 2L }
caller <- function(bins = NULL) { bins <<- (bins <- 1L); print(callee(bins)) }
z <- caller()

## case: backticked formal: (bins <- 1L) ->> bins drops the forwarded default
# no-diag
callee <- function(`bins` = 30) { if (`bins` == 1) 1L else 2L }
caller <- function(bins = NULL) { (bins <- 1L) ->> bins; print(callee(bins)) }
z <- caller()

## case: backticked formal: bins <- (bins <<- 1L) drops the forwarded default
# no-diag
callee <- function(`bins` = 30) { if (`bins` == 1) 1L else 2L }
caller <- function(bins = NULL) { bins <- (bins <<- 1L); print(callee(bins)) }
z <- caller()

## case: backticked formal: bins <- (1L ->> bins) drops the forwarded default
# no-diag
callee <- function(`bins` = 30) { if (`bins` == 1) 1L else 2L }
caller <- function(bins = NULL) { bins <- (1L ->> bins); print(callee(bins)) }
z <- caller()

## case: backticked formal: 1L -> bins drops the forwarded default
# no-diag
callee <- function(`bins` = 30) { if (`bins` == 1) 1L else 2L }
caller <- function(bins = NULL) { 1L -> bins; print(callee(bins)) }
z <- caller()

## case: backticked formal: print({ bins <- 1L; ... }) drops the forwarded default
# no-diag
callee <- function(`bins` = 30) { if (`bins` == 1) 1L else 2L }
caller <- function(bins = NULL) { print({ bins <- 1L; callee(bins) }) }
z <- caller()

## case: a conditional rebinding before the call drops the forwarded default
# no-diag
callee <- function(x_range, bins = 30) {
if (bins == 1) 1 else 2
}
caller <- function(x, bins = NULL) {
if (is.null(bins)) bins <- 30L
callee(range, bins)
}
z <- caller(c(0, 1))

## case: a loop variable rebinding drops the forwarded default
# no-diag
callee <- function(x_range, bins = 30) {
if (bins == 1) 1 else 2
}
caller <- function(x, bins = NULL) {
for (bins in list(1)) callee(range, bins)
}
z <- caller(c(0, 1))

## case: an assignment in a while condition drops the forwarded default
# no-diag
callee <- function(x_range, bins = 30) {
if (bins == 1) 1 else 2
}
caller <- function(x, bins = NULL) {
while ((bins <- f(x)) > 0) break
callee(range, bins)
}
z <- caller(c(0, 1))

## case: zero-length conditions are errors
# expect: RY001, RY001
a <- if (character(0) == "a") 1 else 2
b <- if (numeric(0) > 1) 1 else 2

## case: integer plus double is well typed
# no-diag
1L + 2.0

## case: an integer if condition gets the coercion nudge
# expect: RY003
if (1L) print(1)

## case: an integer-or-double union condition gets the coercion nudge
# expect: RY003
x <- if (runif(1) > 0.5) 1L else 2.0
if (x) print(1)

## case: a union with a list member is an invalid condition
# expect: RY001
# A scalar character member does not make a union invalid. A list member does.
x <- if (runif(1) > 0.5) 1L else list("a")
if (x) print(1)

## case: a NULL if condition is an error
# expect: RY001
if (NULL) print(1)

## case: a non-coercible character if condition is an error
# expect: RY001
if ("x") print(1)

## case: an integer while condition gets the coercion nudge
# expect: RY003
n <- 1L
while (n) n <- 0L

## case: a length-2 logical if condition is an error
# expect: RY002
if (c(TRUE, FALSE)) print(1)

## case: character literal "TRUE" is a valid condition
# no-diag
# R coerces `if` and `while` conditions beyond logical scalars. A raw scalar
# coerces numerically. A complex scalar coerces by OR over its real and
# imaginary parts (0+0i is FALSE, 0+1i is TRUE). A character scalar
# coerces only when its text is one of eight accepted spellings.
if ("TRUE") print(1)

## case: character literal "T" is a valid condition
# no-diag
if ("T") print(1)

## case: character literal "true" is a valid condition
# no-diag
if ("true") print(1)

## case: character literal "True" is a valid condition
# no-diag
if ("True") print(1)

## case: character literal "F" is a valid condition
# no-diag
if ("F") print(1)

## case: character literal "false" is a valid condition
# no-diag
if ("false") print(1)

## case: hex-escaped literal that decodes to "TRUE" is a valid condition
# no-diag
if ("\x54RUE") print(1)

## case: octal-escaped literal that decodes to "FALSE" is a valid condition
# no-diag
if ("\106ALSE") print(1)

## case: raw string literal "TRUE" is a valid condition
# no-diag
if (r"(TRUE)") print(1)

## case: parenthesized character literal is a valid condition
# no-diag
if ((("TRUE"))) print(1)

## case: Sys.getenv() value as if condition stays silent
# no-diag
# The value of a computed string is unknown statically, so R may accept it.
if (Sys.getenv("FLAG")) print(1)

## case: Sys.getenv() binding as if condition stays silent
# no-diag
flag <- Sys.getenv("OTHER")
if (flag) print(1)

## case: Sys.getenv() value as while condition stays silent
# no-diag
while (Sys.getenv("LOOP")) {
  break
}

## case: raw scalar condition is valid
# no-diag
if (as.raw(1)) print(1)

## case: raw zero scalar condition is valid
# no-diag
if (as.raw(0)) print(1)

## case: complex scalar binding is a valid condition
# no-diag
z <- complex(real = 1)
if (z) print(1)

## case: complex scalar from vector() is a valid condition
# no-diag
if (vector("complex", 1)) print(1)

## case: complex scalar with an imaginary part is a valid condition
# no-diag
if (complex(real = 0, imaginary = 1)) print(1)

## case: complex scalar with zero components is a valid condition
# no-diag
if (complex(real = 0, imaginary = 0)) print(1)

## case: imaginary literal 2+3i as condition stays silent
# no-diag
# Imaginary literals type as opaque, so these stay silent by that path.
if (2+3i) print(1)

## case: imaginary literal 0+1i as condition stays silent
# no-diag
if (0+1i) print(1)

## case: imaginary literal 0+0i as condition stays silent
# no-diag
if (0+0i) print(1)

## case: NA_complex_ condition stays silent
# no-diag
# R errors at runtime here ("argument is not interpretable as logical").
# The value decides, not the mode, and ry does not track NA values, so
# silence is uncertainty, not a validity claim.
if (NA_complex_) print(1)

## case: length-2 logical if condition is an error
# expect: RY002
if (c(TRUE, FALSE)) print(1)

## case: length-2 integer if condition is an error
# expect: RY001
if (c(1L, 2L)) print(1)

## case: length-2 double if condition is an error
# expect: RY001
if (c(1, 2)) print(1)

## case: length-2 logical while condition is an error
# expect: RY001
while (c(TRUE, FALSE)) { break }

## case: length-2 integer while condition is an error
# expect: RY001
while (c(1L, 2L)) { break }

## case: length-2 double while condition is an error
# expect: RY001
while (c(1, 2)) { break }

## case: non-coercible character literal condition is an error
# expect: RY001
# R rejects character text outside the eight accepted spellings, including
# "NA", plus multi-element, zero-length, NULL, and list conditions.
if ("x") print(1)

## case: mixed-case "tRue" condition is an error
# expect: RY001
if ("tRue") print(1)

## case: numeric-looking "1" condition is an error
# expect: RY001
if ("1") print(1)

## case: empty string condition is an error
# expect: RY001
if ("") print(1)

## case: "NA" string condition is an error
# expect: RY001
if ("NA") print(1)

## case: two-element character condition is an error
# expect: RY001
if (c("TRUE", "FALSE")) print(1)

## case: zero-length character condition is an error
# expect: RY001
if (character(0)) print(1)

## case: zero-length complex condition is an error
# expect: RY001
if (vector("complex", 0)) print(1)

## case: two-element complex condition is an error
# expect: RY001
if (vector("complex", 2)) print(1)

## case: list condition is an error
# expect: RY001
if (list(TRUE)) print(1)

## case: NULL condition is an error
# expect: RY001
if (NULL) print(1)

## case: an unbound variable is reported
# expect: RY010
y <- undefined_thing

## case: a local length() gets the coercion nudge
# expect: RY003
# A local `length` replaces base length(), so its integer return is an ordinary integer condition.
length <- function(x) 1L
if (length(c(1, 2))) 1

## case: base length() as condition skips the coercion nudge
# no-diag
if (length(c(1, 2))) 1

## case: sum() over a local is.* function gets the coercion nudge
# expect: RY003
# A locally defined `is.*` function is not a base predicate.
is.value <- function(x) 1L
if (sum(is.value(x))) 1

## case: sum() over an alias of is.na skips the coercion nudge
# no-diag
predicate <- is.na
x <- c(1, NA)
if (sum(predicate(x))) 1

## case: sum() over is.na skips the coercion nudge
# no-diag
x <- c(1, NA)
if (sum(is.na(x))) 1

## case: a local sum() over is.na gets the coercion nudge
# expect: RY003
sum <- function(x) 1L
x <- c(1, NA)
if (sum(is.na(x))) 1

## case: base sum() over is.na skips the coercion nudge
# no-diag
x <- c(1, NA)
if (sum(is.na(x))) 1

## case: match() as condition gets the coercion nudge
# expect: RY003
# A missing match is NA_integer_, so it is not a never-NA count.
if (match(1L, 2L)) 1

## case: length() as condition skips the coercion nudge
# no-diag
x <- c(1, 2, 3)
if (length(x)) 1

## case: nrow() as condition skips the coercion nudge
# no-diag
d <- data.frame(a = 1)
if (nrow(d)) 1

## case: ncol() as condition skips the coercion nudge
# no-diag
d <- data.frame(a = 1)
if (ncol(d)) 1

## case: NROW() as condition skips the coercion nudge
# no-diag
l <- list(1)
if (NROW(l)) 1

## case: NCOL() as condition skips the coercion nudge
# no-diag
l <- list(1)
if (NCOL(l)) 1

## case: nobs() as condition skips the coercion nudge
# no-diag
fit <- NULL
if (nobs(fit)) 1

## case: vctrs::vec_size() as condition skips the coercion nudge
# no-diag
library(vctrs)
x <- c(1, 2, 3)
if (vec_size(x)) 1

## case: a loop-carried binding read before its assignment in a for body is bound
# no-diag
n <- function() {
  for (i in 1:3) {
    if (i > 1) print(acc)
    acc <- i
  }
}

## case: a for loop accumulator initialized before the loop is bound
# no-diag
x <- 1:3
total <- 0L
for (i in x) { total <- total + i }

## case: a for loop accumulator first assigned in the loop is bound
# no-diag
x <- 1:3
for (i in x) { total <- total + i }

## case: a loop-carried binding read before its assignment in a while body is bound
# no-diag
keep_going <- TRUE
while (keep_going) {
  print(acc)
  acc <- 1L
}

## case: a loop-carried binding read before its assignment in a repeat body is bound
# no-diag
repeat {
  print(acc)
  acc <- 1L
  break
}

## case: a for loop read in the opposite branch arm is prebound
# expect: RY010
# Names assigned anywhere in a loop body count as bound in every arm.
# This accepts a recall loss (FactoMineR MFA.R:310).
for (i in 1:3) {
  if (is.null(tab.comp)) {
    QuantiAct <- i
  } else {
    print(QuantiAct)
  }
}

## case: a while loop read in the opposite branch arm is prebound
# expect: RY010, RY010
while (keep_going) {
  if (flag == TRUE) {
    value <- 1L
  } else {
    print(value)
  }
}

## case: a for loop read in the else arm of an iteration-dependent condition is prebound
# no-diag
for (i in 1:3) {
  if (i > 1) {
    acc <- i
  } else {
    print(acc)
  }
}

## case: a while loop read in the else arm of a flag condition is prebound
# expect: RY010
while (keep_going) {
  if (flag) {
    value <- 1L
    flag <- FALSE
  } else {
    print(value)
  }
}

## case: an assignment in a nested branch binds the name for the whole loop
# expect: RY010, RY010, RY030
for (g in groups) {
  if (nlevels > 1L) {
    if (conditional.x) {
      COV <- matrix
      COV[is.na(COV)] <- 0
      diag(COV)
    } else {
      COV <- matrix
    }
  } else {
    COV <- matrix
  }
}

## case: a read before assignment outside a loop is unbound
# expect: RY010
f <- function() {
  print(n)
  n <- 1L
}

## case: && on a length-2 logical warns
# expect: RY032
x <- c(TRUE, FALSE)
bad <- x && TRUE

## case: & on a length-2 logical is fine
# no-diag
x <- c(TRUE, FALSE)
ok <- x & TRUE

## case: && on scalars is fine
# no-diag
a <- TRUE
b <- FALSE
x <- a && b

## case: character < numeric warns
# expect: RY033
bad <- "hello" < 42

## case: numeric < numeric is fine
# no-diag
bad <- 1 < 2

## case: character < character is fine
# no-diag
x <- "abc" < "xyz"

## case: character == numeric compares formatted strings and does not warn
# no-diag
ok <- "hello" == 1

## case: character != integer does not warn
# no-diag
ok <- "1" != 1L

## case: logical == character does not warn
# no-diag
ok <- TRUE == "array"

## case: a length-1 %in% result is a valid scalar condition
# no-diag
# `x %in% table` has the length of `x`, not of `table`.
x <- "a"
if (x %in% c("a", "b")) print(1)
if (is.character(x) && x %in% c("a", "b")) print(2)

## case: a parameter default types the parameter in the body
# no-diag
f <- function(x = 1L) { x + 1 }
g <- f(2L)

## case: a user function returning character is a character operand
# expect: RY040
text <- function() { "hello" }
y <- text() + 1L

## case: an explicit return() types the user function result
# expect: RY040
f <- function(x = 1L) { return(x * 2) }
y <- f() + "bad"

## case: a loop over a character vector makes the loop variable character
# expect: RY040
for (s in c("a", "b")) { total <- s + 1 }

## case: a magrittr pipe into a call desugars to a well-typed call
# no-diag
result <- c(1, 2, 3) %>% mean()

## case: a two-step magrittr chain is well typed
# no-diag
a <- c(1, 2, 3) %>% mean() %>% round(2)

## case: the native pipe desugars to a well-typed call
# no-diag
a <- c(1, 2, 3) |> mean()

## case: a magrittr pipe into a bare function name is well typed
# no-diag
x <- 1L
y <- x %>% abs

## case: a magrittr pipe with a dot placeholder is well typed
# no-diag
result <- c(1, 2, 3) %>% round(., digits = 2)

## case: the %T>% tee pipe returns its left side
# no-diag
result <- c(1, 2, 3) %T>% print()

## case: a 60-branch else-if chain is checked
# no-diag
f <- function(what) {
  if (what == "a0") { 0 }
  else if (what == "a1") { 1 }
  else if (what == "a2") { 2 }
  else if (what == "a3") { 3 }
  else if (what == "a4") { 4 }
  else if (what == "a5") { 5 }
  else if (what == "a6") { 6 }
  else if (what == "a7") { 7 }
  else if (what == "a8") { 8 }
  else if (what == "a9") { 9 }
  else if (what == "a10") { 10 }
  else if (what == "a11") { 11 }
  else if (what == "a12") { 12 }
  else if (what == "a13") { 13 }
  else if (what == "a14") { 14 }
  else if (what == "a15") { 15 }
  else if (what == "a16") { 16 }
  else if (what == "a17") { 17 }
  else if (what == "a18") { 18 }
  else if (what == "a19") { 19 }
  else if (what == "a20") { 20 }
  else if (what == "a21") { 21 }
  else if (what == "a22") { 22 }
  else if (what == "a23") { 23 }
  else if (what == "a24") { 24 }
  else if (what == "a25") { 25 }
  else if (what == "a26") { 26 }
  else if (what == "a27") { 27 }
  else if (what == "a28") { 28 }
  else if (what == "a29") { 29 }
  else if (what == "a30") { 30 }
  else if (what == "a31") { 31 }
  else if (what == "a32") { 32 }
  else if (what == "a33") { 33 }
  else if (what == "a34") { 34 }
  else if (what == "a35") { 35 }
  else if (what == "a36") { 36 }
  else if (what == "a37") { 37 }
  else if (what == "a38") { 38 }
  else if (what == "a39") { 39 }
  else if (what == "a40") { 40 }
  else if (what == "a41") { 41 }
  else if (what == "a42") { 42 }
  else if (what == "a43") { 43 }
  else if (what == "a44") { 44 }
  else if (what == "a45") { 45 }
  else if (what == "a46") { 46 }
  else if (what == "a47") { 47 }
  else if (what == "a48") { 48 }
  else if (what == "a49") { 49 }
  else if (what == "a50") { 50 }
  else if (what == "a51") { 51 }
  else if (what == "a52") { 52 }
  else if (what == "a53") { 53 }
  else if (what == "a54") { 54 }
  else if (what == "a55") { 55 }
  else if (what == "a56") { 56 }
  else if (what == "a57") { 57 }
  else if (what == "a58") { 58 }
  else if (what == "a59") { 59 }
  else { stop("nope") }
}

## case: the magrittr dot resolves inside a subscript of the dot
# no-diag
# `mtcars %>% .[.$mpg > 20, ]` selects 14 rows in R.
a <- mtcars %>% .[.$mpg > 20, ]
b <- mtcars %>% .$mpg[.$cyl > 4]

## case: the magrittr dot is unbound in a native pipe
# expect: RY000, RY010
# R rejects `$` as a native pipe right-hand side at parse time.
a <- mtcars |> .$mpg

## case: the native underscore is unbound in a magrittr pipe
# expect: RY010
b <- mtcars %>% _$mpg

## case: an unknown column through the magrittr dot is reported
# expect: RY060
df <- mtcars
bad <- df %>% .$nonexistent

## case: a column extracted through the magrittr dot is double
# no-diag
df <- mtcars
col <- df %>% .$mpg
ok <- col + 1L

## case: two integer if branches join to integer
# expect: RY040
x <- if (TRUE) 1L else 2L
bad <- x + "hello"

## case: arithmetic on a list-or-function if result is an error
# expect: RY040
# An operation on a union errors only when every member errors.
x <- if (TRUE) list(1) else function() { 1 }
bad <- x + 1

## case: an if without else joins its value with NULL
# no-diag
x <- if (TRUE) 1L
y <- x + 1

## case: nested integer if expressions join to integer
# expect: RY040
x <- if (TRUE) { if (FALSE) 1L else 2L } else 3L
bad <- x + "x"

## case: unary minus on character is an error
# expect: RY020
x <- -"hi"

## case: -c("col") in j on a possible data.table is a column drop
# no-diag
# data.table reads `-<character>` and `!<character>` in `j` as column drops,
# and `!<character>` or `!<list>` in `i` as a not-join. Base R has neither.
f <- function(design) design[, -c("dob", "eol")]

## case: -c("col") in j on a data.table-classed value is a column drop
# no-diag
dt <- structure(list(a = 1), class = c("data.table", "data.frame"))
u <- dt[, -c("a")]

## case: !"key" in i on a possible data.table is a not-join
# no-diag
f <- function(dt) dt[!"key"]

## case: !list() in i on a possible data.table is a not-join
# no-diag
f <- function(dt) dt[!list(k = "a")]

## case: !c("col") in j on a possible data.table is a column drop
# no-diag
f <- function(dt) dt[, !c("a")]

## case: -c("x") on a base vector is an error
# expect: RY020
v <- c("x", "y")
u <- v[-c("x")]

## case: !"x" on a base vector is an error
# expect: RY021
v <- c("x", "y")
u <- v[!"x"]

## case: -c("a") in j on a base data.frame is an error
# expect: RY020
df <- data.frame(a = 1, b = 2)
u <- df[, -c("a")]

## case: !list() on a base list is an error
# expect: RY021
l <- list(a = 1, b = 2)
u <- l[!list(a = 1)]

## case: -c("a") in the i slot is an error
# expect: RY020
# data.table reads `-<character>` as a drop only in `j`.
f <- function(dt) dt[-c("a")]

## case: -c("a") in [[ is an error
# expect: RY020
f <- function(dt) dt[[-c("a")]]

## case: a named j = -c("a") argument is a column drop
# no-diag
# R-verified: `dt[j = -c("a")]` drops the column.
f <- function(dt) dt[j = -c("a")]

## case: -c("a") passed to the unused drop argument is an error
# expect: RY020
# R-verified: data.table warns that the ignored `drop` argument will become an error.
f <- function(dt) dt[, drop = -c("a")]

## case: .SDcols = !c("a") is a selection inversion
# no-diag
# R-verified: data.table documents `.SDcols = !cols` and `.SDcols = -cols` as inversions.
f <- function(dt) dt[, .SD, .SDcols = !c("a")]

## case: .SDcols = -c("a") is a selection inversion
# no-diag
f <- function(dt) dt[, .SD, .SDcols = -c("a")]

## case: -c("a") in the by slot is an error
# expect: RY020
# R-verified: positional slot 2 binds `by`, where `-` and `!` error.
f <- function(dt) dt[, .N, -c("a")]

## case: !c("a") in the by slot is an error
# expect: RY021
f <- function(dt) dt[, .N, !c("a")]

## case: unary minus in a callback inside j is an error
# expect: RY020
# R-verified: the callback runs in its own frame, so `-s` errors.
f <- function(dt) dt[, sapply(c("a"), function(s) -s)]

## case: unary minus in a foreach body inside j is an error
# expect: RY020
# A foreach `%do%` body runs in its own environment, so `-c("a")` errors in R.
f <- function(dt) dt[, foreach(i = 1:2) %do% -c("a")]

## case: 1:10 plus character is an error
# expect: RY040
x <- 1:10
bad <- x + "hello"

## case: rep() result plus character is an error
# expect: RY040
x <- rep(c(1, 2), 3)
bad <- x + "hello"

## case: seq() result plus character is an error
# expect: RY040
x <- seq(1, 10, 2)
bad <- x + "hello"

## case: a 1:10 function result plus character is an error
# expect: RY040
f <- function() 1:10
g <- f()
bad <- g + "hello"

## case: calling a double value is an error
# expect: RY070
x <- 42
y <- x(10)

## case: calling a character value is an error
# expect: RY070
x <- "paste"
y <- x(1)

## case: calling a function is fine
# no-diag
f <- function() 1L
y <- f()

## case: calling an unknown value stays silent
# no-diag
y <- some_unknown_thing(10)

## case: calling a numeric literal is an error
# expect: RY070
y <- 42()

## case: calling NULL is an error
# expect: RY070
y <- NULL()

## case: calling an index expression stays silent
# no-diag
lst <- list(function() 1)
y <- lst[[1]]()

## case: $ on an integer vector is an error
# expect: RY061
# R: "$ operator is invalid for atomic vectors".
x <- 1:10
val <- x$col

## case: $ on a character vector is an error
# expect: RY061
x <- c("a", "b")
val <- x$col

## case: $ on a list is fine
# no-diag
x <- list(a = 1)
val <- x$a

## case: a $ method on a classed atomic value may write to the caller
# no-diag
`$.widget` <- function(x, name) { assign('marker', 1L, envir=parent.frame()); 2L }; f <- function() { marker <- 'before'; x <- structure(1L, class='widget'); out <- x$field; marker + 1L }; f()

## case: a $ method on a classed union may write to the caller
# no-diag
`$.widget` <- function(x,name) { assign('marker',1L,envir=parent.frame()); 2L }; f <- function(flag) { marker <- 'before'; x <- structure(if(flag) 1L else 'payload',class='widget'); out <- x$field; marker+1L }; f(TRUE); f(FALSE)

## case: a $<- method on a classed integer may write to the caller
# no-diag
`$<-.widget` <- function(x,name,value) { assign('marker',1L,envir=parent.frame()); list(saved=value) }; f <- function(flag) { marker <- 'before'; x <- structure(1L,class='widget'); x$field <- 3L; marker+1L; x$saved }; f(TRUE); f(FALSE)

## case: a $<- method on a classed union may write to the caller
# no-diag
`$<-.widget` <- function(x,name,value) { assign('marker',1L,envir=parent.frame()); list(saved=value) }; f <- function(flag) { marker <- 'before'; x <- structure(if(flag) 1L else 'payload',class='widget'); x$field <- 3L; marker+1L; x$saved }; f(TRUE); f(FALSE)

## case: $<- on a plain integer is an error
# expect: RY061
x <- 1L; x$field <- 3L

## case: $ on a data.frame is fine
# no-diag
val <- mtcars$mpg

## case: $ on an unknown value stays silent
# expect: RY010
x <- some_unknown_thing
val <- x$col

## case: $ on an all-atomic union is an error
# expect: RY061
x <- if (runif(1) > 0.5) 1L else "x"
x$field

## case: $ on a union with a list member is fine
# no-diag
x <- if (runif(1) > 0.5) 1L else list(field = 1)
x$field

## case: an early return joins the trailing if type
# no-diag
process <- function(req, raw = FALSE) {
  if (req == 204) return(TRUE)
  if (raw) req else jsonlite::fromJSON("x")
}
use <- function(x) process(x)$config

## case: $ on an all-atomic joined return is an error
# expect: RY061
a <- function(req, raw) {
  if (req == 204) return(TRUE)
  if (raw) 1 else 2
}
b <- function(x) a(x, FALSE)$k

## case: is.null(x) || is.na(x) on a parameter warns
# expect: RY032
f <- function(x) is.null(x) || is.na(x)

## case: length(x) && x == 1L on a parameter warns
# expect: RY032
f <- function(x) length(x) && x == 1L

## case: is.na() on a reassigned scalar parameter is fine
# no-diag
f <- function(x) { x <- TRUE; is.null(x) || is.na(x) }

## case: is.null(x) || 'value' %in% x is a scalar test
# no-diag
f <- function(x) is.null(x) || 'value' %in% x

## case: is.null(x) || !('value' %in% x) is a scalar test
# no-diag
f <- function(x) is.null(x) || !('value' %in% x)

## case: length(x) == 1L proves x scalar for is.na()
# no-diag
f <- function(x) length(x) == 1L && is.na(x)

## case: 1 == length(x) proves x scalar
# no-diag
f <- function(x) 1 == length(x) && x == ''

## case: base::length(x) == 1 proves x scalar
# no-diag
f <- function(x) base::length(x) == 1 && x == ''

## case: x %in% 'value' after is.null() warns
# expect: RY032
f <- function(x) is.null(x) || x %in% 'value'

## case: length(x) > 0 does not prove x scalar
# expect: RY032
f <- function(x) length(x) > 0 && x == ''

## case: length(x) == 2L does not prove x scalar
# expect: RY032
f <- function(x) length(x) == 2L && is.na(x)

## case: a local length() does not prove x scalar
# expect: RY032
length <- function(x) 1L; f <- function(x) length(x) == 1L && is.na(x)

## case: a length parameter does not prove x scalar
# expect: RY032
f <- function(x, length) length(x) == 1L && is.na(x)

## case: other::length() does not prove x scalar
# expect: RY032
f <- function(x) other::length(x) == 1L && is.na(x)

## case: a project length method breaks the length(x) == 1L guard
# expect: RY032
# A `length.<class>` method can report 1 for a longer value, so the
# guard does not prove the operand scalar (docs/scalar-guards.md).
length.disguised <- function(x) 1L
f <- function(x) length(x) == 1L && x == 1L

## case: a project length method breaks the guard for is.na()
# expect: RY032
length.disguised <- function(x) 1L
f <- function(x) length(x) == 1L && is.na(x)

## case: a classed parameter default breaks the length guard
# expect: RY032
# The default names a class a length method could attach to.
length.disguised <- function(x) 1L
f <- function(x = structure(c(1L, 2L), class = "disguised")) length(x) == 1L && x == 1L

## case: an unclassed parameter default does not save the length guard
# expect: RY032
# Callers can still pass a classed value.
length.disguised <- function(x) 1L
f <- function(x = 1L) length(x) == 1L && x == 1L

## case: reassignment inside the guarded operand breaks the length guard
# expect: RY032
f <- function(x) length(x) == 1L && { x <- c(1, 2); x == 1L }

## case: length(x) == 1L proves x scalar without a length method
# no-diag
f <- function(x) length(x) == 1L && x == 1L

## case: length(x) == 1L proves x scalar with a scalar default
# no-diag
f <- function(x = 1L) length(x) == 1L && x == 1L

## case: a loop-carried value does not keep its initial empty length
# no-diag
quote <- raw()
for (x in as.raw(c(1, 2))) {
if (length(quote)) { if (x == quote) print(x) }
quote <- x
}

## case: bindings at a while break reach the loop exit
# no-diag
f <- function(flag) { scale <- 1L; trial <- list(); while (TRUE) { trial$score <- 1L; if (flag) break; trial <- list(alpha=2) }; if (flag) scale <- 1 + abs(trial$score) else scale <- abs(trial$score); if (scale > 0) TRUE }

## case: bindings at a repeat break reach the loop exit
# no-diag
f <- function(flag) { scale <- 1L; trial <- list(); repeat { trial$score <- 1L; if (flag) break; trial <- list(alpha=2) }; if (flag) scale <- 1 + abs(trial$score) else scale <- abs(trial$score); if (scale > 0) TRUE }

## case: bindings at a for break reach the loop exit
# no-diag
f <- function(flag) { scale <- 1L; trial <- list(); for (i in 1:2) { trial$score <- 1L; if (flag) break; trial <- list(alpha=2) }; if (flag) scale <- 1 + abs(trial$score) else scale <- abs(trial$score); if (scale > 0) TRUE }

## case: a masked + operator may rebind a variable before the loop exit
# no-diag
`+` <- function(e1, e2) { assign("x", list(field = 1L), envir = parent.frame()); NULL }
f <- function(flag) {
    x <- 1L
    while (TRUE) {
        if (flag) break
        1 + 2
        break
    }
    if (!flag) x$field
}
f(FALSE)

## case: `n1` <- 42 binds n1 for a later function
# no-diag
`n1` <- 42; f <- function() n1 + 1

## case: `g` <- function binds g for a later function
# no-diag
`g` <- function(x) x + 1; f <- function() { z <- g; z(1) }

## case: a later `g` <- 42 replaces the function value
# no-diag
`g` <- function() 1; `g` <- 42; f <- function() g + 1

## case: n1 <- 2 replaces `n1` <- 1
# no-diag
`n1` <- 1; n1 <- 2; f <- function() n1

## case: `n1` <- 2 replaces n1 <- 1
# no-diag
n1 <- 1; `n1` <- 2; f <- function() n1

## case: `n1` does not bind n2
# expect: RY010
`n1` <- 42; f <- function() n2

## case: a string "`n1`" assignment does not bind n1
# expect: RY010
"`n1`" <- 42; f <- function() n1

## case: `n\x31` does not bind n2
# expect: RY010
`n\x31` <- 42; f <- function() n2

## case: a length-2 condition inside abs() keeps its length
# expect: RY002, RY100
if (abs(c(1, 2) > 0) > 0) 1L

## case: 1:"a" is not typed numeric
# no-diag
# R: 1:"a" errors at the `:` ("NA/NaN argument"), and "1":"3" works.
# Character text is unknown statically, so neither gets a numeric type.
x <- 1:"a"
y <- x + "a"

## case: 1:s with a character s is not typed numeric
# no-diag
s <- "a"
x <- 1:s
y <- x + "a"

## case: 1:"Inf" is not typed numeric
# no-diag
x <- 1:"Inf"
y <- x + "a"

## case: tryCatch(error = h, { ... }) is typed from the expression, not the handler
# no-diag
# Named handlers bind by name, so the first unnamed argument is `expr`
# (pkgload's dev-help.R).
f <- function(topic) {
  ok <- tryCatch(
    error = function(...) FALSE,
    {
      is.character(topic)
    }
  )
  if (ok) 1
}

## case: tryCatch(error = h, expr = x) is typed from expr
# no-diag
f <- function(topic) {
  ok <- tryCatch(error = function(e) FALSE, expr = is.character(topic))
  if (ok) 1
}

## case: tryCatch(x, error = h, finally = cleanup()) is typed from x and h
# no-diag
cleanup <- function() "done"
f <- function(topic) {
  ok <- tryCatch(is.character(topic), error = function(e) FALSE, finally = cleanup())
  if (ok) 1
}

## case: tryCatch(error = h, "b") + 1L is character arithmetic
# expect: RY040
f <- function() {
  x <- tryCatch(error = function(e) "a", "b")
  x + 1L
}
