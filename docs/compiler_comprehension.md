# Array Comprehension Implementation Strategy

Since we can't mutate locals (Jsonnet is functional), we need a different approach.

## Stack-Based Loop Strategy

For `[expr for x in source_array]`:

Stack layout during loop:
```
[source_array, result_array, counter, length]
```

Loop body:
1. Check if counter < length, jump to end if false
2. Get element: source_array[counter]
3. Bind element to loop variable (as local)
4. Evaluate expr -> value
5. Append value to result_array -> new_result_array
6. Clean up: remove loop variable local
7. Increment counter: counter + 1 -> new_counter
8. Update stack: [source_array, new_result_array, new_counter, length]
9. Jump back to loop start

After loop:
- Pop counter, length
- Leave result_array on stack

This requires careful stack management with Dup/Swap/Pop.
