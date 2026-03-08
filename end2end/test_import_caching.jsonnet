assert (local a = import "import_caching_target.libsonnet";
local b = import "import_caching_target.libsonnet";
local valA = a.x;
local valB = b.x;
if valA == valB then true else error "failed"); true