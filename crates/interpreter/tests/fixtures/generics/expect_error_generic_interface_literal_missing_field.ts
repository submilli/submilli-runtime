// An object literal argument missing a field of the generic interface it is
// passed as is reported once, naming the interface, as tsc reports it.
// expect-error: object literal is missing required field `v` of type `Box`
// expect-error: object literal is missing required field `b` of type `Pair`
// expect-error-count: 2
interface Box<T> {
  v: T;
}

interface Pair<A, B> {
  a: A;
  b: B;
}

function unbox<T>(b: Box<T>): T {
  return b.v;
}

function first<A, B>(p: Pair<A, B>): A {
  return p.a;
}

function main(): void {
  unbox({});
  first({ a: 1 });
}
