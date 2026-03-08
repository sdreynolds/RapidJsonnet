assert (local m = import "import_basic_target.libsonnet";
local val = m.value;
if val == 42 then true else error "failed"); true
