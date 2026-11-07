// expect-error: does not satisfy the constraint
class Box<T extends number> { constructor(value: T) {} }
function main(): void { new Box("bad"); }
