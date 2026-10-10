// An assignment used as a value gets the checks of the assignment statement.
// expect-error: cannot assign to const binding `c`

function main(): void {
  const c = 1;
  const y = (c = 2);
  console.log(y);
}
