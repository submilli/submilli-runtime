// test262: test/built-ins/Array/prototype/reverse/S15.4.4.8_A1_T1.js
// Adapted: the `reverse() === x` reference-identity checks become element
// checks on x itself (reverse mutates in place; === is structural here).

function main(): void {
  const empty: number[] = [];
  const reversedEmpty = empty.reverse();
  assert(reversedEmpty.length === 0, "x = []; x.reverse() is x (empty)");

  const x = [1, 2];
  x.reverse();

  assert(x[0] === 2, "x = [1,2]; x.reverse(); x[0] === 2");
  assert(x[1] === 1, "x = [1,2]; x.reverse(); x[1] === 1");
  assert(x.length === 2, "x = [1,2]; x.reverse(); x.length === 2");
}
