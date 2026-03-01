local x = import "does-not-exist.libsonnet";
if 1 == 1 then true else error "failed"