// test262: test/built-ins/Map/prototype/has/normalizes-zero-key.js

function main(): void {
  const map = new Map<number, number>();

  assertSameValue(map.has(-0), false);
  assertSameValue(map.has(0), false);

  map.set(-0, 42);
  assertSameValue(map.has(-0), true);
  assertSameValue(map.has(0), true);

  map.clear();

  map.set(0, 42);
  assertSameValue(map.has(-0), true);
  assertSameValue(map.has(0), true);
}
