// Each level of a deep chain also refers back to its first interface. A level
// measured while that interface is still being measured is remembered for as
// long as what it assumed about the interface holds, which keeps the
// comparison linear and still finds `T` contravariant at the end.
// A declaration that refers back to itself, directly or through another, in
// a flipped position is measured again until its variances settle, so `T`
// is invariant in `S` and `P`. A doubly linked chain, where every level is
// in one such group, is walked again only from its first level, so it too
// stays linear. A method's callback parameter is compared strictly, as in
// tsc, so `Source<T>` is covariant.
// expect-error: expected `L5<number>`, got `L5<1>`
// expect-error: expected `S<number>`, got `S<1>`
// expect-error: expected `P<number>`, got `P<1>`
// expect-error: expected `D0<number>`, got `D0<number | string>`
// expect-error: expected `Source<number>`, got `Source<number | string>`
// expect-error-count: 5
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

interface D0<T> {
  v: () => T;
  next: D1<T>;
}

interface D1<T> {
  v: () => T;
  next: D2<T>;
  prev: D0<T>;
}

interface D2<T> {
  v: () => T;
  next: D3<T>;
  prev: D1<T>;
}

interface D3<T> {
  v: () => T;
  next: D4<T>;
  prev: D2<T>;
}

interface D4<T> {
  v: () => T;
  next: D5<T>;
  prev: D3<T>;
}

interface D5<T> {
  v: () => T;
  next: D6<T>;
  prev: D4<T>;
}

interface D6<T> {
  v: () => T;
  next: D7<T>;
  prev: D5<T>;
}

interface D7<T> {
  v: () => T;
  next: D8<T>;
  prev: D6<T>;
}

interface D8<T> {
  v: () => T;
  next: D9<T>;
  prev: D7<T>;
}

interface D9<T> {
  v: () => T;
  next: D10<T>;
  prev: D8<T>;
}

interface D10<T> {
  v: () => T;
  next: D11<T>;
  prev: D9<T>;
}

interface D11<T> {
  v: () => T;
  next: D12<T>;
  prev: D10<T>;
}

interface D12<T> {
  v: () => T;
  next: D13<T>;
  prev: D11<T>;
}

interface D13<T> {
  v: () => T;
  next: D14<T>;
  prev: D12<T>;
}

interface D14<T> {
  v: () => T;
  next: D15<T>;
  prev: D13<T>;
}

interface D15<T> {
  v: () => T;
  next: D16<T>;
  prev: D14<T>;
}

interface D16<T> {
  v: () => T;
  next: D17<T>;
  prev: D15<T>;
}

interface D17<T> {
  v: () => T;
  next: D18<T>;
  prev: D16<T>;
}

interface D18<T> {
  v: () => T;
  next: D19<T>;
  prev: D17<T>;
}

interface D19<T> {
  v: () => T;
  next: D20<T>;
  prev: D18<T>;
}

interface D20<T> {
  v: () => T;
  prev: D19<T>;
}

function takeD(v: D0<number>): void {}

function narrowerD(v: D0<1>): void {
  takeD(v);
}

function widerD(v: D0<number | string>): void {
  takeD(v);
}

interface Source<T> {
  subscribe(listener: (value: T) => void): void;
}

function narrowerSource(s: Source<1>): Source<number> {
  return s;
}

function widerSource(s: Source<number | string>): Source<number> {
  return s;
}

function main(): void {}
