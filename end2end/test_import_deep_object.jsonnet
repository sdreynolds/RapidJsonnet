local m = import "import_deep_object_target.libsonnet";
local val = m.a.b.c.d;
if val == 100 then true else error "failed"