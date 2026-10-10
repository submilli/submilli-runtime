// A method that writes, or takes an element, would have to accept every
// member's element type at once, so it isn't offered on a union of arrays. A
// callback's array argument is the receiver, so it only permits reading too.
// expect-error: cannot call `push` on `number[] | string[]`
// expect-error: cannot call `includes` on `number[] | string[]`
// expect-error: cannot call `push` on `readonly (number | string)[]`
// expect-error: cannot call `push` on `readonly (number | string | boolean)[]`
// expect-error-count: 4
function update(column: string[] | number[]): void {
  column.push(3);
  const found = column.includes(2);
  column.forEach((cell, i, all) => {
    all.push(5);
  });
}

function intoTuple(row: [number, string] | boolean[]): void {
  row.some((cell, i, all) => {
    all.push(true);
    return false;
  });
}

function main(): void {}
