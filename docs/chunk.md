# Creating the Chunk Byte Code
The chunk struct is a collection of

- a `&str` called `source_id` that will be used with `ariadne` library
- a vector of `u8` called `code` that contains either the `opcode` or `operand` that will be consumed by the virtual machine
- a vector of a `Span` used to keep map an index in `code` vector to the location in a `source_id`
- a vector of `Value` struct that maps to primitive values

## Vector of `Span`
This represents the offsets in a file or repl session that generated the `opcode` or `operand` for the index in the `code` vector. This data is used to report a compile time or runtime error when processing all the values in `code`.

## Value struct
To start, this struct will only be an f64 and therefore will be a single `type` alias. This will be expanded upon later.
