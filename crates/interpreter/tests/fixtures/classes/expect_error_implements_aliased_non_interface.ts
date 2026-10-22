// expect-error-count: 4
// expect-error: a class can only `implements` an interface or object type, but `number` is neither
// expect-error: a class can only `implements` an interface or object type, but `A | B` is neither
// expect-error: a class can only `implements` an interface or object type, but `() => number` is neither
// expect-error: a class can only `implements` an interface or object type, but `K` is neither
// The other side of admitting an aliased interface or object type: an alias whose
// body is neither must still be rejected, and the message names the body rather
// than the alias, since the body is what makes it ineligible. An object type is
// accepted (see implements_object_type_alias.ts).
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
type TUnion = A | B;
type TFn = () => number;
type TClass = K;

class C1 implements TNum {
  x: number = 1;
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
