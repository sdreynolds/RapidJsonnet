assert ({
  name: "Alice",           // identifier key
  "first-name": "Alice",   // string key (with dash)
  age: 30,                 // identifier key  
  "is-active": true        // string key (with dash)
}) == {"age":30.0,"first-name":"Alice","is-active":true,"name":"Alice"}; {"age":30.0,"first-name":"Alice","is-active":true,"name":"Alice"}