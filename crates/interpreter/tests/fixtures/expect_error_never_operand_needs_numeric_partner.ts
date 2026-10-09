// A `never` operand still needs a partner the operator could take: like tsc,
// arithmetic and ordering reject a string beside it, though `+` accepts one.
// A `never` target can't take a compound assignment's result back either, which
// is reported once, against the result, even where `never` is only the narrowed
// type of a `number` binding.
// expect-error-count: 5
// expect-error: `-` not defined for `never` and `string`
// expect-error: `<` not defined for `never` and `string`
// expect-error: expected `never`, got `number`
// expect-error: expected `never`, got `string`
// expect-error: expected `never`, got `number`
function f(x: never, y: never): void {
  const difference = x - "s";
  const before = x < "s";
  y -= x;
  y += "s";
}

function g(): void {
  let z: number = 1;
  if (typeof z === "string") {
    z -= 1;
  }
}

function main(): void {}
