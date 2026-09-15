// test262: test/built-ins/Map/prototype/size/returns-count-of-present-values-before-after-set-delete.js

function main(): void {
  const map = new Map<number, number>();

  assertSameValue(map.size, 0, "The value of `map.size` is `0`");

  map.set(1, 1);
  assertSameValue(map.size, 1, "The value of `map.size` is `1`, after executing `map.set(1, 1)`");

  map.delete(1);
  assertSameValue(map.size, 0, "The value of `map.size` is `0`, after executing `map.delete(1)`");
}
