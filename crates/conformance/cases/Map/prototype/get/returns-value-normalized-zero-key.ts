// test262: test/built-ins/Map/prototype/get/returns-value-normalized-zero-key.js

function main(): void {
  let map = new Map<number, number>();

  map.set(0, 42);
  assertSameValue(map.get(-0), 42);

  map = new Map<number, number>();
  map.set(-0, 43);
  assertSameValue(map.get(0), 43);
}
