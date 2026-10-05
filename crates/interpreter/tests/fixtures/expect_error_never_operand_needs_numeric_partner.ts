// A `never` operand still needs a partner the operator could take: like tsc,
// arithmetic and ordering reject a string beside it, though `+` accepts one.
// expect-error-count: 2
// expect-error: `-` not defined for `never` and `string`
// expect-error: `<` not defined for `never` and `string`
function f(x: never): void {
  const difference = x - "s";
  const before = x < "s";
}

function main(): void {}
