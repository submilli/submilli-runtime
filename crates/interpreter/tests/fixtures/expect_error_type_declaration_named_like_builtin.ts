// A type declaration may not take a built-in type's name, as in TypeScript
// (TS2414, TS2427, TS2457): `class number {}` would read as the built-in in
// some positions and the class in others.
// expect-error: `number` is a built-in type and can't be used as a class name
// expect-error: `string` is a built-in type and can't be used as an interface name
// expect-error: `boolean` is a built-in type and can't be used as a type alias name
// expect-error: `unknown` is a built-in type and can't be used as an enum name
class number {
  x: number = 1;
}
interface string {
  a: number;
}
type boolean = number;
enum unknown {
  A,
}

function main(): void {}
