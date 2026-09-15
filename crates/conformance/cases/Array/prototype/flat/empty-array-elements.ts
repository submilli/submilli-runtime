// test262: test/built-ins/Array/prototype/flat/empty-array-elements.js
// Adapted: the `a = {}` object element becomes a number — element types are
// homogeneous here, and the point (empty arrays vanish) is unchanged.

function main(): void {
  const empty: number[][] = [];
  assertCompareArray(empty.flat(), [], "[].flat() must return []");

  const emptyPair: number[][] = [[], []];
  assertCompareArray(emptyPair.flat(), [], "[ [], [] ].flat() must return []");

  const oneTail: number[][] = [[], [1]];
  assertCompareArray(oneTail.flat(), [1], "[ [], [1] ].flat() must return [1]");

  const pairTail: number[][] = [[], [1, 7]];
  assertCompareArray(pairTail.flat(), [1, 7], "[ [], [1, 7] ].flat() must return [1, 7]");
}
