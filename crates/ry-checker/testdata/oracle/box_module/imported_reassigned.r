box::use(./reexport[foo])
base::assign("foo", function() 1L)
box::export(foo)
