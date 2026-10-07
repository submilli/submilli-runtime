// Each level of a deep chain also refers back to its first interface. A level
// measured while that interface is still being measured is remembered for as
// long as what it assumed about the interface holds, which keeps the
// comparison linear and still finds `T` contravariant at the end.
// A declaration that refers back to itself, directly or through another, in
// a flipped position is measured again until its variances settle, so `T`
// is invariant in `S` and `P`.
// expect-error: expected `L5<number>`, got `L5<1>`
// expect-error: expected `S<number>`, got `S<1>`
// expect-error: expected `P<number>`, got `P<1>`
// expect-error-count: 3
interface L0<T> {
  a: L1<T>;
  b: L1<T>;
  c: L0<T>;
}

interface L1<T> {
  a: L2<T>;
  b: L2<T>;
  c: L0<T>;
}

interface L2<T> {
  a: L3<T>;
  b: L3<T>;
  c: L0<T>;
}

interface L3<T> {
  a: L4<T>;
  b: L4<T>;
  c: L0<T>;
}

interface L4<T> {
  a: L5<T>;
  b: L5<T>;
  c: L0<T>;
}

interface L5<T> {
  a: L6<T>;
  b: L6<T>;
  c: L0<T>;
}

interface L6<T> {
  a: L7<T>;
  b: L7<T>;
  c: L0<T>;
}

interface L7<T> {
  a: L8<T>;
  b: L8<T>;
  c: L0<T>;
}

interface L8<T> {
  a: L9<T>;
  b: L9<T>;
  c: L0<T>;
}

interface L9<T> {
  a: L10<T>;
  b: L10<T>;
  c: L0<T>;
}

interface L10<T> {
  a: L11<T>;
  b: L11<T>;
  c: L0<T>;
}

interface L11<T> {
  a: L12<T>;
  b: L12<T>;
  c: L0<T>;
}

interface L12<T> {
  a: L13<T>;
  b: L13<T>;
  c: L0<T>;
}

interface L13<T> {
  a: L14<T>;
  b: L14<T>;
  c: L0<T>;
}

interface L14<T> {
  a: L15<T>;
  b: L15<T>;
  c: L0<T>;
}

interface L15<T> {
  a: L16<T>;
  b: L16<T>;
  c: L0<T>;
}

interface L16<T> {
  a: L17<T>;
  b: L17<T>;
  c: L0<T>;
}

interface L17<T> {
  a: L18<T>;
  b: L18<T>;
  c: L0<T>;
}

interface L18<T> {
  a: L19<T>;
  b: L19<T>;
  c: L0<T>;
}

interface L19<T> {
  a: L20<T>;
  b: L20<T>;
  c: L0<T>;
}

interface L20<T> {
  a: L21<T>;
  b: L21<T>;
  c: L0<T>;
}

interface L21<T> {
  a: L22<T>;
  b: L22<T>;
  c: L0<T>;
}

interface L22<T> {
  a: L23<T>;
  b: L23<T>;
  c: L0<T>;
}

interface L23<T> {
  a: (x: T) => void;
  b: (x: T) => void;
  c: L0<T>;
}

function take(x: L5<number>): void {}

function wider(v: L5<number | string>): void {
  take(v);
}

function narrower(v: L5<1>): void {
  take(v);
}

interface S<T> {
  get: () => T;
  cmp: (o: S<T>) => void;
}

interface P<T> {
  get: () => T;
  q: Q<T>;
}

interface Q<T> {
  cmp: (o: P<T>) => void;
}

function takeS(v: S<number>): void {}

function narrowerS(v: S<1>): void {
  takeS(v);
}

function takeP(v: P<number>): void {}

function narrowerP(v: P<1>): void {
  takeP(v);
}

function main(): void {}
