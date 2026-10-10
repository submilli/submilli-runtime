// expect-error: expected `3`, got `4`
function check(index: number): void {
  const values: number[] = [3];
  if (values[0] === 3) {
    values[index] = 4;
    assert(values[0] === 4);
  }
}
