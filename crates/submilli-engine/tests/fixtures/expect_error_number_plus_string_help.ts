// expect-error: `+` not defined for `number` and `string`
// expect-error: convert the string with `Number(...)`, `parseInt(...)`, or `parseFloat(...)`
function main(): number {
  const count: string = "3";
  return 1 + count;
}
