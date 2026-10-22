// A tuple element that holds `null` in one member discriminates in a switch
// default and under truthiness too, and an `unknown` equal to a value takes
// the value's type, as tsc narrows them.
type P4 = [null, string] | [boolean, number];
function k(v: P4): string {
  switch (v[0]) {
    case null: return v[1];
    default: return String(v[1] + 1);
  }
}
function n(v: P4): string {
  if (!v[0]) return "falsy";
  return String(v[1] + 1);
}
function g(x: unknown, y: number): number {
  if (x === y) return x + 1;
  return 0;
}
function h(x: unknown, y: string, z: number[]): string {
  if (x === y) return x.toUpperCase();
  if (x === z) return "same array";
  return "no";
}
function main(): void {
  const arr = [1];
  assert(k([null, "s"]) === "s");
  assert(k([true, 1]) === "2");
  assert(n([null, "s"]) === "falsy");
  assert(n([false, 2]) === "falsy");
  assert(n([true, 2]) === "3");
  assert(g(4, 4) === 5);
  assert(g("4", 4) === 0);
  assert(h("a", "a", arr) === "A");
  assert(h(arr, "b", arr) === "same array");
  assert(h(1, "b", arr) === "no");
}
