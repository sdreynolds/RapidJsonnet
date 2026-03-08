assert ({
  person: {name: "Alice", age: 30},
  location: "home"
} + {
  person: {age: 31, job: "engineer"},
  status: "active"
}) == {"location":"home","person":{"age":31.0,"job":"engineer"},"status":"active"}; {"location":"home","person":{"age":31.0,"job":"engineer"},"status":"active"}