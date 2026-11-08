// A value fits `T["a"]` only where every instantiation's property holds it,
// and a write may not contradict a bound naming another type parameter or a
// `readonly` property (TS2322, TS2540).
// expect-error: expected `T["a"]`, got `number`
// expect-error: expected `U`, got `number`
// expect-error: cannot assign to readonly property `a`
function read<T extends { a: number }>(value: T): T["a"] { return 5; }
function nested<U extends string | number, T extends { a: U }>(value: T): void { value["a"] = 5; }
function frozen<T extends { readonly a: number }>(value: T): void { value["a"] = 5; }
function main(): void {}
