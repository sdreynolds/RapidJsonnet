assert (std.setMember(2, [1, 2, 3]) == true &&
std.setMember(4, [1, 2, 3]) == false &&
std.setMember('b', ['a', 'b', 'c']) == true &&
std.setMember('z', ['a', 'b', 'c']) == false &&
std.setMember(1, []) == false); true
