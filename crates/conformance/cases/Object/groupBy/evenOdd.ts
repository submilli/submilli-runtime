// test262: test/built-ins/Object/groupBy/evenOdd.js
function main(): void {
  const array = [1, 2, 3];
  const obj = Object.groupBy(array, (i: number): string => i % 2 === 0 ? "even" : "odd");
  assertCompareArray(Object.keys(obj), ["odd", "even"], "group keys");
  assertCompareArray(obj["even"]!, [2], "even values");
  assertCompareArray(obj["odd"]!, [1, 3], "odd values");
}
