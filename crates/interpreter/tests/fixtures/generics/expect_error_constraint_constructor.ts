// expect-error: expected `number`, got `"bad"`
class Box<T extends number> { constructor(value: T) {} }
function main(): void { new Box("bad"); }
