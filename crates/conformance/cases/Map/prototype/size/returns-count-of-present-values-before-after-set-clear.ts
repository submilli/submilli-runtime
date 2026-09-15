// test262: test/built-ins/Map/prototype/size/returns-count-of-present-values-before-after-set-clear.js

function main(): void {
  const map = new Map<number, number>();

  assertSameValue(map.size, 0, "The value of `map.size` is `0`");

  map.set(1, 1);
  map.set(2, 2);
  assertSameValue(map.size, 2, "The value of `map.size` is `2`");

  map.clear();
  assertSameValue(map.size, 0, "The value of `map.size` is `0`, after executing `map.clear()`");
}
