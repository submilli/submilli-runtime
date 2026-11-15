type Q = { a: number; b: number[] };
type P = { a: number; b: number[]; extra: string };
function mk(): P { return { a: 1, b: [2], extra: "e" }; }
let g: Q | string[] = ["s"];
function reset(): void { g = { a: 9, b: [3] }; }
function main(): void {
  g = mk();
  reset();
  const actual = g;
  if (Array.isArray(actual)) throw new Error("reset must leave an object");
  assert(actual.a === 9, "read live reassigned object");
  assert(actual.b.length === 1, "read live array");
}
