// test262: test/built-ins/Map/prototype/forEach/deleted-values-during-foreach.js
// Adapted: the {value, key} result objects are recorded in parallel arrays.

function main(): void {
  const map = new Map<string, number>();
  map.set("foo", 0);
  map.set("bar", 1);

  let count = 0;
  let keys: string[] = [];
  let values: number[] = [];

  map.forEach((value: number, key: string): void => {
    if (count === 0) {
      map.delete("bar");
    }
    keys.push(key);
    values.push(value);
    count++;
  });

  assertSameValue(keys.length, 1);
  assertSameValue(keys[0], "foo");
  assertSameValue(values[0], 0);
}
