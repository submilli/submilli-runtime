// `String()` of a Set or Map prints its tag, as JavaScript does.
function main(): void {
  const set = new Set<number>([1]);
  const map = new Map<string, number>([["a", 1]]);
  assert(String(set) === "[object Set]", "a Set");
  assert(String(map) === "[object Map]", "a Map");
  assert(String({ a: 1 }) === "[object Object]", "a plain object is unchanged");
  console.log(String(set), String(map));
}
