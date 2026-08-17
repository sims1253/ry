# Purpose: one verified type/operator diagnostic per section. Every line
# below was confirmed against `ry check` before being written into this
# file; the rule code in each comment is what the editor should show.
#
# Expected diagnostics:
#   RY031 x2 - logical ops on non-coercible operands
#   RY032    - && with a length-2 operand
#   RY033    - character vs numeric comparison
#   RY034 x2 - == and != against NA
#   RY040    - arithmetic on incompatible types
#   RY041    - non-divisible recycling
#   RY042    - arithmetic on a factor
#   RY060    - data-frame column not in schema
#   RY061    - $ on an atomic vector
#   RY070    - calling a non-function value
#   RY002    - if condition of length 2
#   RY099    - discarded one-arm if value

flags <- c(TRUE, FALSE, TRUE)

bad_logical_and <- "x" & 1            # RY031: `character` and `double`
bad_logical_or <- TRUE | "yes"        # RY031: `logical` and `character`

bad_scalar_logical <- flags && TRUE   # RY032: length-3 operand

bad_mode_compare <- "small" < 42      # RY033: byte-wise lexicographic

prices <- c(3.5, NA, 4.25)
na_eq <- prices == NA                 # RY034: use is.na()
na_ne <- NA_real_ != prices           # RY034: use !is.na()

bad_arith <- "latte" + 1L             # RY040: character + integer

bad_recycle <- c(1, 2) + c(1, 2, 3)   # RY041: lengths 2 and 3

sizes <- factor(c("small", "medium"))
bad_factor <- sizes + 1               # RY042: arithmetic on factor

menu <- data.frame(item = c("espresso"), price = c(2.5))
missing_col <- menu$steamed_milk      # RY060: schema has item, price

sizes_plain <- c("small", "medium")
dollar_atomic <- sizes_plain$milk     # RY061: $ on character atomic

total <- 42
call_a_number <- total(1)             # RY070: `total` is double

discarded <- function(units) {
  if (units > 0L) units * 2           # RY099: value produced but discarded
  units + 1L
}

cond_length <- function() {
  if (flags) 1L else 2L               # RY002: length-3 condition
}
