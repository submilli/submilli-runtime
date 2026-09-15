// test262: test/built-ins/Map/prototype/clear/clear-map.js
// expect-fail: new Map(entries) with mixed-type entries fails Wasm validation — a number/boolean element inside a union-typed tuple is emitted unboxed (f64/i32) where the entry list expects a boxed ref
// Adapted: the Symbol-keyed entry in m2 is dropped by design.

function main(): void {
  const m1 = new Map<string | number, string | number>([
    ["foo", "bar"],
    [1, 1],
  ]);
  const m2 = new Map<string | number, string | number>();
  const m3 = new Map<string, string>();
  m2.set("foo", "bar");
  m2.set(1, 1);

  m1.clear();
  m2.clear();
  m3.clear();

  assertSameValue(m1.size, 0);
  assertSameValue(m2.size, 0);
  assertSameValue(m3.size, 0);
}
