// expect-error: `+` not defined for `string` and `number`
// expect-error: wrap the number with `String(...)`
function main(): string {
  const count: number = 3;
  return "Count: " + count;
}
