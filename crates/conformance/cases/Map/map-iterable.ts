// test262: test/built-ins/Map/map-iterable.js

function main(): void {
  const m = new Map<string, number>([
    ["attr", 1],
    ["foo", 2],
  ]);

  assertSameValue(m.size, 2, "The value of `m.size` is `2`");
  assertSameValue(m.get("attr"), 1);
  assertSameValue(m.get("foo"), 2);
}
