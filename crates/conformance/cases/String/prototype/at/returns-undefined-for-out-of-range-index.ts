// test262: test/built-ins/String/prototype/at/returns-undefined-for-out-of-range-index.js
// Adapted: prototype-method reflection omitted.

function main(): void {
  let s = "";

  assertSameValue(s.at(-2), undefined, 's.at(-2) must return undefined'); // wrap around the end
  assertSameValue(s.at(0), undefined, 's.at(0) must return undefined');
  assertSameValue(s.at(1), undefined, 's.at(1) must return undefined');
}
