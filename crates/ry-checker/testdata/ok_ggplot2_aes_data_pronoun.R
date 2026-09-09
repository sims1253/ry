# no-diag
# `.data` inside aes()/vars()/qplot() has no usable data argument at the
# call site: the first supplied argument is itself data-masked, so it
# cannot serve as the fallback mask source. The pronoun must stay
# opaque and cannot adopt a sibling argument's atomic type (#383). A
# preceding double, character, or call-typed sibling must not become the
# mask. A named `data` argument the signature does not declare as a
# mask source stays conservative: unsupported, never an error.
ribbon1 <- ggplot2::geom_ribbon(
  ggplot2::aes(ymin = -Inf, ymax = .data$se.lo),
  alpha = 0.1
)
ribbon2 <- geom_ribbon(aes(ymin = 0, ymax = .data$curve_y))
edge <- ggraph::geom_edge_arc(
  aes(
    alpha = as.numeric(.data$Component == "Correlation"),
    label = .data$Label_Correlation,
    color = .data$Coefficient
  ),
  strength = 0.1
)
# A character sibling must not leak its mode either.
chr_sibling <- ggplot2::aes(colour = "red", x = .data$mpg)
# `.data` in the leading argument is evaluated before any sibling type
# exists and stays quiet in both directions.
leading <- ggplot2::aes(x = .data$mpg, colour = "red")
only <- ggplot2::aes(ymin = .data$se)
plain <- ggplot(df, aes(x = .data$x, y = .data$y))
keep <- ggplot(df, aes(x = .data$x)) + geom_point(aes(colour = .data$grp))
# Leading `...` data-masked formals have no data argument either.
vars_q <- ggplot2::vars(.data$mpg)
# qplot's first supplied argument is itself masked and the named `data`
# argument is not a declared mask source, so the mask stays unknown and
# `$` access is never an error.
q <- qplot(x = .data$mpg, y = .data$wt, data = mtcars)
# Checker-behavior retention controls (not R-oracle claims): APIs whose
# leading formal is the data argument keep their mask -- positional,
# named, and data_mask_source-declaring signatures.
w1 <- with(mtcars, .data$mpg)
w2 <- with(data = mtcars, .data$mpg)
t1 <- transform(mtcars, k = .data$mpg * 2)
m1 <- lm(mpg ~ wt, data = mtcars)
