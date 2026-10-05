// A method that takes an element would have to accept every member's element
// type at once, so it isn't offered on a union of arrays.
// expect-error: cannot call `push` on `number[] | string[]`
// expect-error: cannot read field `includes`
// expect-error-count: 2
function update(column: string[] | number[]): void {
  column.push(3);
  const found = column.includes(2);
}

function main(): void {}
