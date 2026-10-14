// test262: test/built-ins/Object/groupBy/groupLength.js
function main(): void {
  const arr = ["hello", "test", "world"];
  const obj = Object.groupBy(arr, (i: string): number => i.length);
  assertCompareArray(Object.keys(obj), ["4", "5"], "group keys");
  assertCompareArray(obj["5"]!, ["hello", "world"], "length five");
  assertCompareArray(obj["4"]!, ["test"], "length four");
}
