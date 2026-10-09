// test262: test/built-ins/Map/prototype/forEach/iterates-values-added-after-foreach-begins.js

function main(): void {
  const map = new Map<string, number>();
  map.set("foo", 0);
  map.set("bar", 1);

  let count = 0;
  let keys: string[] = [];
  let values: number[] = [];

  map.forEach((value: number, key: string): void => {
    if (count === 0) {
      map.set("baz", 2);
    }
    keys.push(key);
    values.push(value);
    count++;
  });

  assertSameValue(count, 3);
  assertSameValue(map.size, 3);

  assertSameValue(keys[0], "foo");
  assertSameValue(values[0], 0);

  assertSameValue(keys[1], "bar");
  assertSameValue(values[1], 1);

  assertSameValue(keys[2], "baz");
  assertSameValue(values[2], 2);
}
