// test262: test/built-ins/RegExp/S15.10.2.10_A1.1_T1.js
// Test262Error throws become asserts.

function main(): void {
  const one = /\t/.exec("\u0009");
  assert(one !== null, "\\t matches U+0009");
  if (one === null) {
    return;
  }
  const t1 = one.match;
  assertSameValue(t1, "\u0009", "single tab");

  const two = /\t\t/.exec("a\u0009\u0009b");
  assert(two !== null, "\\t\\t matches two tabs");
  if (two === null) {
    return;
  }
  const t2 = two.match;
  assertSameValue(t2, "\u0009\u0009", "double tab");
}
