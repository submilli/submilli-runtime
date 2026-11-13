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
function nested<U>(n: N<U[]>): U[] {
  return n.v;
}
function main(): void {
  const text: N<string> = { v: "hello", w: 7 };
  const number: N<number> = { v: 42, w: 9 };
  const values: N<string[]> = { v: ["first", "second"], w: 2 };
  assert(f(text) === 7, "fixed property");
  assert(g(text) === "hello", "string property");
  assert(g(number) === 42, "number property");
  assert(nested(values)[1] === "second", "nested type argument");
}
