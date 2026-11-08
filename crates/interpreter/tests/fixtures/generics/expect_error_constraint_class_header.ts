// expect-error: class Box<T extends number>
class Box<T extends number> { constructor(public value: T) {} }
function main(): void { const box: Box = new Box(1); }
