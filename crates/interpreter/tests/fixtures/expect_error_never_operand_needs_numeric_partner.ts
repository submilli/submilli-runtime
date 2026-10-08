// A `never` operand still needs a partner the operator could take: like tsc,
// arithmetic and ordering reject a string beside it, though `+` accepts one.
// A `never` target can't take a compound assignment's result back either, which
// is reported once, against the result.
// expect-error-count: 4
// expect-error: `-` not defined for `never` and `string`
// expect-error: `<` not defined for `never` and `string`
// expect-error: expected `never`, got `number`
// expect-error: expected `never`, got `string`
function f(x: never, y: never): void {
  const difference = x - "s";
  const before = x < "s";
  y -= x;
  y += "s";
}

function main(): void {}
