local utils = import "rules/tests/utils.libsonnet";

{
  message: utils.greet("World"),
}
