assert (local y = "this is great";
local x = {
  awesome: true,
  nestedObj: {
    anotherNest: 45,
    someString: y,
  }
};
x) == {"awesome":true,"nestedObj":{"anotherNest":45.0,"someString":"this is great"}}; {"awesome":true,"nestedObj":{"anotherNest":45.0,"someString":"this is great"}}
