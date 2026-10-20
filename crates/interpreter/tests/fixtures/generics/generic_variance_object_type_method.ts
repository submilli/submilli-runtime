// A method declared in an object type (`{ f(x: T): void }`) is measured as
// an interface method is: its parameters are contravariant, so a parameter
// of such a method inside a method's parameter makes `T` covariant, and one
// a level deeper, inside a callback, makes it contravariant. tsc compares
// those parameters both ways instead.
interface A<T> {
  m(p: { f(x: T): void }): void;
}

interface D<T> {
  m(cb: (p: { f(x: T): void }) => void): void;
}

class Impl implements A<1> {
  m(p: { f(x: 1): void }): void {
    p.f(1);
  }
}

class Calls implements D<number> {
  m(cb: (p: { f(x: number): void }) => void): void {}
}

function a1(a: A<1>): A<number> {
  return a;
}

function d1(d: D<number>): D<1> {
  return d;
}

function main(): void {
  const impl: A<1> = new Impl();
  const calls: D<number> = new Calls();
  assert(a1(impl) === impl && d1(calls) === calls, "each direction its variance allows");
}
