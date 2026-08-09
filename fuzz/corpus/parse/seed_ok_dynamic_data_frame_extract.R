# no-diag
dynamic_data_frame_column <- function(p_col = "p") {
  zph_table <- as.data.frame(list())
  zph_table$variable <- "GLOBAL"
  results_df <- zph_table[zph_table$variable != "GLOBAL", , drop = FALSE]
  p_value <- results_df[[p_col]]
  p_value < 0.05
}
