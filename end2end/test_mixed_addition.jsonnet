assert ({
  numbers: 10 + 5,
  strings: "hello" + " world", 
  objects: {a: 1} + {b: 2}
}) == {"numbers":15.0,"objects":{"a":1.0,"b":2.0},"strings":"hello world"}; {"numbers":15.0,"objects":{"a":1.0,"b":2.0},"strings":"hello world"}