box::use(./text_function[foo])
base::assign("foo", function() 1L)
box::export(foo)
