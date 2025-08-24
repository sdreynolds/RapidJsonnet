# String Interning
The Lua programming language interns all of it's strings. this makes comparasion cheap. In Jsonnet, it is very common to compare strings for equality. Propose how to intern
  strings in this implementaion

# Global String pool
The global string pool will implement a a Mark and Sweep garabage collection. The collector frequency will automatically adjust ased on the live size of the heap. The pool will keep track of the number of bytes in managed memory and when it goes above a threshold it will trigger a Mark and Sweep colection. There are two fields to keep traack off

- bytes_allocated -- number of bytes used by the virtual machine
- next_garbage_collection -- threshold to trigger the next collection

`next_garbage_collection` will be start at 1024 * 1024. Whenever a string is allocated by the String pool, increase he `bytes_allocated` by the size of the string. Add a `dellocate_string` method to remove a string from the pool. This method will descrease `bytes_allocated` by the size of the string removed. Whenver garabage collection runs, `next_garbage_collection ` will be set to `bytes_allocated` * 2.


# Getting roots for garbage collection
To start a garbage collection pass and to create the grey list the roots will have to be collected. The roots for this program come from the virtual machines stack. For each string in the stack, add it to the gray list. Then iterate over the gray list to mark the strings as black. This will be a future integration point when there are more objects types than strings.

After all the gray list is processed and all the strings that are in use are marked as black, deallocate all the strings in the pool that are not marked and blackened.

# Triggering a garbage collection
A garbage collection will be triggered by either `#[cfg(env_var_value = "stress_gc")]` is set at build time or when the `bytes_allocated` exceeds `next_garbage_collection`. After the garbage collection process update `bytes_allocated` by the growth factor of 2
