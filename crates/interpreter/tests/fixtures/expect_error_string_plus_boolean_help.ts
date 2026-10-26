// expect-error: `+` not defined for `string` and `boolean`
// expect-error: write `String(done)` to concatenate
function main(): string {
  const done: boolean = Math.random() < 2;
  return "Done: " + done;
}
