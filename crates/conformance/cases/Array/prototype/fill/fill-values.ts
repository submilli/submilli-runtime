// test262: test/built-ins/Array/prototype/fill/fill-values.js
// Adapted: the `[0, 0].fill()` row is dropped — `value` is a required
// parameter here (no undefined to default to).

function main(): void {
  const empty: number[] = [];
  assertCompareArray(empty.fill(8), [], "[].fill(8) must return []");

  assertCompareArray([0, 0, 0].fill(8), [8, 8, 8],
    "[0, 0, 0].fill(8) must return [8, 8, 8]",
  );

  assertCompareArray([0, 0, 0].fill(8, 1), [0, 8, 8],
    "[0, 0, 0].fill(8, 1) must return [0, 8, 8]",
  );

  assertCompareArray([0, 0, 0].fill(8, 4), [0, 0, 0],
    "[0, 0, 0].fill(8, 4) must return [0, 0, 0]",
  );

  assertCompareArray([0, 0, 0].fill(8, -1), [0, 0, 8],
    "[0, 0, 0].fill(8, -1) must return [0, 0, 8]",
  );

  assertCompareArray([0, 0, 0].fill(8, 0, 1), [8, 0, 0],
    "[0, 0, 0].fill(8, 0, 1) must return [8, 0, 0]",
  );

  assertCompareArray([0, 0, 0].fill(8, 0, -1), [8, 8, 0],
    "[0, 0, 0].fill(8, 0, -1) must return [8, 8, 0]",
  );

  assertCompareArray([0, 0, 0].fill(8, 0, 5), [8, 8, 8],
    "[0, 0, 0].fill(8, 0, 5) must return [8, 8, 8]",
  );
}
