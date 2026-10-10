// Field initializers run in declaration order, before the constructor assigns
// its parameter properties (ES2022, as TypeScript reports with TS2729). A
// closure reads later and an inherited field is already set, so both are fine.
// expect-error: property `b` is used before its initialization
// expect-error: property `n` is used before its initialization
// expect-error: property `q` is used before its initialization
// expect-error-count: 3
class A {
  x: string = this.b.toUpperCase();
  later: number = this.n + 1;
  n: number = 2;
  f: () => string = () => this.b;
  constructor(public b: string) {}
}
class P { constructor(public p: number) {} }
class C extends P {
  z: number = this.q;
  w: number = this.p;
  constructor(public q: number) { super(q); }
}
function main(): void {}
