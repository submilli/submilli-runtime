// A function-typed property in an object type is compared strictly, as in
// tsc, so this generic interface relates no other instantiation of itself.
// expect-error: expected `E<number>`, got `E<1>`
// expect-error-count: 1
interface E<T> {
  m(p: { f: (x: T) => T }): void;
}

function e1(a: E<1>): E<number> {
  return a;
}

function main(): void {}
