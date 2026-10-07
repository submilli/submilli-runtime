// Each level of a deep chain also refers back to its first interface, so a
// level measured while that first interface is still being measured skips
// the reference; it is remembered for the rest of that measurement, which
// keeps the comparison linear and still finds `T` contravariant at the end.
// expect-error: expected `L5<number>`, got `L5<1>`
// expect-error-count: 1
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

function main(): void {}
