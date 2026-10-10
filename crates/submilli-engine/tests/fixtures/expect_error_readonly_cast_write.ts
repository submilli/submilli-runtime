// expect-error: cannot assign to an element of `readonly number[]`
function main(): void {
  const ro: readonly number[] = [1];
  const value = ro as readonly number[] | null;
  if (value !== null) { value[0] = 2; }
}
