// A member that breaks its type's index signature is reported once, at the
// declaration (tsc: TS2411), not again at each place the type is named.
// expect-error-count: 2
// expect-error: property `foo` of type `string` does not satisfy string index value type `number[]`
// expect-error: property `bar` of type `string` does not satisfy string index value type `number`
interface I8 { [x: string]: number[]; foo: string; }
interface X { y: I8; }
type A = { [x: string]: number; bar: string };

function main(): void {
  let z: I8 = null as unknown as I8;
  let a: A[] = [];
}
