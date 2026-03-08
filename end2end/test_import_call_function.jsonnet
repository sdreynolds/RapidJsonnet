assert (local m = import "import_call_function_target.libsonnet";
local val = m.add(10, 5);
if val == 15 then true else error "failed"); true