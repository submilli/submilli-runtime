// expect-error: duplicate declaration of type `T`
// expect-error: declared here as a type parameter
// expect-error: duplicate declaration of `E`
// expect-error: a type declared in a block can't use `typeof` on the local `n`
// expect-error: duplicate declaration of interface `A`
// expect-error: can't share a name with a parameter of its function
// A function's type parameters share its body's scope, an enum is also a
// value, and a local type can't read a parameter's type with `typeof`.

function clash<T>(x: T): void {
  interface T { v: number; }
}

function enumAndValue(): void {
  const E = 1;
  enum E { A }
}

function typeofParam(n: number): void {
  type N = typeof n;
}

function twice<T>(t: T): void {
  interface A { x: T; }
  interface A { y: number; }
}

function enumAndParam(E: number): void {
  enum E { A }
}

function main(): void {}
