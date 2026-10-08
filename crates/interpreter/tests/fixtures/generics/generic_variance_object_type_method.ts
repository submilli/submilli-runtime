// A method declared in an object type (`{ f(x: T): T }`) is measured as an
// interface method is, as in tsc: its parameters are compared both ways, so
// a generic interface using one relates its instantiations in both
// directions, unlike one using a function-typed property.
interface A<T> {
  m(p: { f(x: T): T }): void;
}

interface D<T> {
  m(cb: (p: { f(x: T): void }) => void): void;
}

class Impl implements A<number> {
  m(p: { f(x: number): number }): void {}
}

function a1(a: A<1>): A<number> {
  return a;
}

function a2(a: A<number>): A<1> {
  return a;
}

function d1(a: D<1>): D<number> {
  return a;
}

function main(): void {
  const impl: A<number> = new Impl();
  assert(a1(a2(impl)) === impl, "both directions");
}
