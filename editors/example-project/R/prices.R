# Purpose: CLEAN reference file. Unicode identifiers (a non-ASCII name and
# a backtick-quoted emoji name) must produce ZERO diagnostics; this file is
# the "no squiggles" control when flipping between editors.
#
# Expected diagnostics: none.

TAX_RATE <- 1.07  # cross-file constant: menu.R and resolution.R read this

#' Add sales tax to a price.
price_with_tax <- function(price, rate = TAX_RATE) {
  round(price * rate, digits = 2L)
}

# Non-ASCII identifier: ordinary binding, ordinary use.
café_latte_price <- 4.5
café <- function(base_price) {
  price_with_tax(base_price) + 0.25
}

# Backtick-quoted emoji identifier: must survive parse + name resolution.
`☕` <- 3.75
`📈` <- c(1.1, 1.25, 1.4)

with_emoji <- `☕` * length(`📈`)

cup_total <- function(n_cups) {
  price_with_tax(n_cups * `☕`)
}
