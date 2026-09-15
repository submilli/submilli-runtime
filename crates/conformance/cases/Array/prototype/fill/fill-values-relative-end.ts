// test262: test/built-ins/Array/prototype/fill/fill-values-relative-end.js

function main(): void {
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
