# oracle: must-pass
# oracle-facts: df_empty_col df_named_vec t_df_num t_df_chr t_vec_int
# Soundness pins for #923: zero-length data.frame columns, 1-arg named atomic
# data.frame(), and t(data.frame) returning an unclassed matrix.
df_empty <- data.frame(a = integer(0))
df_empty_col <- df_empty$a
df_named_vec <- data.frame(c(a = 1, b = 2))
t_df_num <- t(data.frame(a = 1, b = 2L))
t_df_chr <- t(data.frame(a = 1, b = "x"))
t_vec_int <- t(1:3)
