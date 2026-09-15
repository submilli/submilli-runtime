// test262: test/built-ins/Array/prototype/flatMap/depth-always-one.js

function main(): void {
  assertCompareArray(
    [1, 2].flatMap((e: number): number[] => [e, e * 2]),
    [1, 2, 2, 4],
    "[1, 2].flatMap(e => [e, e * 2]) must return [1, 2, 2, 4]",
  );

  const result = [1, 2, 3].flatMap((ele: number): number[][] => [[ele * 2]]);
  assertSameValue(result.length, 3, "The value of result.length is expected to be 3");
  assertCompareArray(result[0], [2], "The value of result[0] is expected to be [2]");
  assertCompareArray(result[1], [4], "The value of result[1] is expected to be [4]");
  assertCompareArray(result[2], [6], "The value of result[2] is expected to be [6]");
}
