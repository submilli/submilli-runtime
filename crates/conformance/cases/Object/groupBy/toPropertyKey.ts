// test262: test/built-ins/Object/groupBy/toPropertyKey.js
// Adaptation: unknown[] represents the heterogeneous array; toString returns "1" because this language requires string-valued conversion hooks.
function main(): void {
  const stringable = { toString: (): string => "1" };
  const array: unknown[] = [1, "1", stringable];
  const obj = Object.groupBy(array, (v: unknown): unknown => v);
  assertCompareArray(Object.keys(obj), ["1"], "group keys");
  assertCompareArray(obj["1"]!, [1, "1", stringable], "coerced keys share one group");
}
