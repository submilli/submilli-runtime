// Generic validators need an environment descriptor even without generic calls.
interface N<T> {
  v: T;
  w: number;
}
function f<U>(n: N<U>): number {
  return n.w;
}
function g<U>(n: N<U>): U {
  return n.v;
}
function main(): number {
  return 0;
}
