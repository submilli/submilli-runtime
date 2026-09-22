// expect-error: unary `-` not defined for `string`
// Annotated so the receiver renders as `string`; an unannotated `const`
// infers the literal type `"42"`.
function main(): void {
  const s: string = "42";
  const n = -s;
}
