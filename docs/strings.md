# String Implementation
For Jsonnet, there is a need to store String values. Today, we have number, null, true and false and our task is to extend that beyond those values. The task is the following

- Extend the Value enum to include String values
- Extend `Compiler` when parsing expressions to handle the `String` token and create the `String` value in the `constants` vector
- Inspect the `VirtualMachine` to make sure it can handle loading `String` values to and from the `stack`
