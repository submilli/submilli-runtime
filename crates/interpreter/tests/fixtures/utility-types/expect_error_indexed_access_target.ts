// A value fits `T["a"]` only where every instantiation's property holds it,
// and a write may not contradict a bound naming another type parameter or a
// `readonly` property (TS2322, TS2540).
// expect-error: expected `T["a"]`, got `number`
// expect-error: expected `U`, got `number`
// expect-error: cannot assign to readonly property `a`
// expect-error: cannot assign to readonly property `r`
// expect-error: cannot assign to readonly property `g`
// expect-error: field `a` does not exist on `Pick<T, K>
// expect-error: `Pick<T, "length">` has no members to read
function read<T extends { a: number }>(value: T): T["a"] { return 5; }
function nested<U extends string | number, T extends { a: U }>(value: T): void { value["a"] = 5; }
function frozen<T extends { readonly a: number }>(value: T): void { value["a"] = 5; }
class C { readonly r: number = 1; get g(): number { return 1; } }
function classField<T extends C>(value: T): void { value["r"] = 5; }
function getter<T extends C>(value: T): void { value["g"] = 5; }
function picked<T extends { a: number; b: string }, K extends "a" | "b">(value: Pick<T, K>): number { return value.a; }
function length<T extends number[]>(value: Pick<T, "length">): number { return value.length; }
function main(): void {}
