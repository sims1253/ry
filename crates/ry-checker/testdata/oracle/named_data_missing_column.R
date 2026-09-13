# oracle: must-flag
d <- data.frame(x = 1L)
dplyr::mutate(new = .data$missing, .data = d)
