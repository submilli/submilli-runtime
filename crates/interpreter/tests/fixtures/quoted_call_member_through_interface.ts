// A quoted method named "@call" is an ordinary method: a call through an
// interface reaches it, as in TypeScript, while a value of a call-signature
// interface is called as the function it is. SUB-1524 tracks the two sharing
// one spelling.
interface X {
  "@call"(v: number): number;
}
class C implements X {
  "@call"(v: number): number {
    return v * 3;
  }
}
class Base implements X {
  base: number = 1;
  "@call"(v: number): number {
    return this.base + v;
  }
}
class Derived extends Base {
  "@call"(v: number): number {
    return 100 + super["@call"](v);
  }
}
class Inherits extends Base {}
interface Sink {
  "@call"(v: number): void;
}
function main(): void {
  const x: X = new C();
  assert(x["@call"](3) === 9);
  assert(new C()["@call"](4) === 12);
  const o: X = {
    "@call"(v: number): number {
      return v + 100;
    },
  };
  assert(o["@call"](5) === 105);
  const derived: X = new Derived();
  const inherits: X = new Inherits();
  assert(derived["@call"](1) === 102 && inherits["@call"](2) === 3);
  let seen = 0;
  const sink: Sink = {
    "@call"(v: number): void {
      seen = v;
    },
  };
  sink["@call"](7);
  assert(seen === 7);
}
