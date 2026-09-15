// test262: test/built-ins/JSON/parse/text-negative-zero.js
// The original's final `JSON.parse(-0)` (ToString coercion of a number
// argument) is dropped: parse takes only a string here.

function main(): void {
  const a: number = JSON.parse("-0") as number;
  assertSameValue(a, -0);
  const b: number = JSON.parse(" \n-0") as number;
  assertSameValue(b, -0);
  const c: number = JSON.parse("-0  \t") as number;
  assertSameValue(c, -0);
  const d: number = JSON.parse("\n\t -0\n   ") as number;
  assertSameValue(d, -0);
}
