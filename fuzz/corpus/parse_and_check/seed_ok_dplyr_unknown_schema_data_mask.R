# no-diag
build_tte_data <- function(adsl, first_event) {
  tte_data <- adsl
  tte_data$TRTDURD <- 10
  tte_data |>
    dplyr::left_join(first_event, by = "USUBJID") |>
    dplyr::mutate(
      event = ifelse(!is.na(has_event), 1, 0),
      time = ifelse(event == 1, ASTDY, TRTDURD),
      time = ifelse(is.na(time), TRTDURD, time)
    ) |>
    dplyr::filter(!is.na(time), !is.na(TRT01A))
}
