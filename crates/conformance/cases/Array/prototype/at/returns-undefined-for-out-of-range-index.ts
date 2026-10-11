// test262: test/built-ins/Array/prototype/at/returns-undefined-for-out-of-range-index.js
// Adapted: prototype-method reflection omitted; the empty array has an element type.

function main(): void {
  const a: number[] = [];

  assertSameValue(a.at(-2), undefined, 'a.at(-2) returns undefined'); // wrap around the end
  assertSameValue(a.at(0), undefined, 'a.at(0) returns undefined');
  assertSameValue(a.at(1), undefined, 'a.at(1) returns undefined');
}
