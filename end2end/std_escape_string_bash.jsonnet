assert (
std.escapeStringBash("hello world") == "'hello world'" &&
std.escapeStringBash("it's here") == "'it'\"'\"'s here'" &&
std.escapeStringBash("") == "''"
); true
