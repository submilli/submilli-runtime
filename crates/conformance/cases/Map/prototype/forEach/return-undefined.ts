// test262: test/built-ins/Map/prototype/forEach/return-undefined.js
// Adapted: the callback returns boolean through a typed arrow; forEach's
// void completion is observed as undefined.

function main(): void {
  const map = new Map<number, number>();

  let result: unknown = map.forEach((): boolean => {
    return true;
  });

  assertSameValue(result, undefined, "Empty map#forEach returns undefined");

  map.set(1, 1);
  result = map.forEach((): boolean => {
    return true;
  });

  assertSameValue(result, undefined, "map#forEach returns undefined");
}
