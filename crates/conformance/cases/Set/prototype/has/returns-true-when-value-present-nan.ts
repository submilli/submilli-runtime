// test262: test/built-ins/Set/prototype/has/returns-true-when-value-present-nan.js
// expect-fail: NaN elements are unfindable — element equality runs through the equals vtable, which uses IEEE === for numbers, not SameValueZero, so has(NaN) is false even after add(NaN)

function main(): void {
  const s = new Set<number>();

  s.add(NaN);

  assertSameValue(s.has(NaN), true, "`s.has(NaN)` returns `true`");
}
