// expect-error: `+` not defined for `string` and `number`
// expect-error: write `String(count)` to concatenate
function main(): string {
  const count: number = 3;
  return "Count: " + count;
}
