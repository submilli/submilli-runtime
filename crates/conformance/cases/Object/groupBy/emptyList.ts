// test262: test/built-ins/Object/groupBy/emptyList.js
function main(): void {
  const original: number[] = [];
  const obj = Object.groupBy(original, (): string => { throw new Error("callback function should not be called"); });
  assertNotSameValue(original, obj, "Object.groupBy returns an object");
  assertSameValue(Object.keys(obj).length, 0, "empty groups");
}
