# no-diag
# A computed target binds its decoded alias opaquely.
box::use(alias = make_path())
value <- alias
box::use(`\x65scaped` = make_path())
value <- escaped
