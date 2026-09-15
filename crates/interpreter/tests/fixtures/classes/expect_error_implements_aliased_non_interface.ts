// expect-error: a class can only `implements` an interface, but `number` is not an interface
// expect-error: a class can only `implements` an interface, but `{ a: number }` is not an interface
// expect-error: a class can only `implements` an interface, but `A | B` is not an interface
// expect-error: a class can only `implements` an interface, but `() => number` is not an interface
// expect-error: a class can only `implements` an interface, but `K` is not an interface
// The other side of admitting an aliased interface: an alias whose body is not
// an interface must still be rejected, and the message names the body rather
// than the alias, since the body is what makes it ineligible.
interface A {
  a(): number;
}
interface B {
  b(): number;
}
class K {
  v: number = 1;
}

type TNum = number;
type TObj = { a: number };
type TUnion = A | B;
type TFn = () => number;
type TClass = K;

class C1 implements TNum {
  x: number = 1;
}
class C2 implements TObj {
  a: number = 1;
}
class C3 implements TUnion {
  a(): number {
    return 1;
  }
}
class C4 implements TFn {
  x: number = 1;
}
class C5 implements TClass {
  v: number = 2;
}

function main(): void {}
