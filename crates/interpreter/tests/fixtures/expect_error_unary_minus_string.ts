// expect-error: unary `-` not defined for `string`
function main(): void {
  const s = "42";
  const n = -s;
}
