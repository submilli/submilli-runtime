// expect-error: variable `value` is used before being assigned
function repeat(first: number, second: number): boolean {
  console.log(first, second);
  return false;
}
function main(): void {
  let value: number;
  while (repeat(value, value = 1)) {}
}
