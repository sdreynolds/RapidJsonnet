# Building the Compiler
For this implementation, we are going to build a compiler that use the Scanner to get tokens out and compile those tokens into a Byte Code virtual machine. To build the compiler there will be a structure called a Parser that has:

1. &Scanner
2. previous_token
3. current_token
4. had_error boolean
5. panic_mode boolean

The Parser will have a method called advance that will set the previous token to the current and then get the next token from the Scanner. If the Scanner returns an error, the Parser will set both had_error and panic_mode to true. If panic_mode wasn't previously true, call ScanError::into_report otherswise it will return.

The Parser will have a method called consume that will take in the expected Token enum and a message string. In the event the Token enum doesn't match the current token, then it will create an error report and return that up to the caller of the consume method.
