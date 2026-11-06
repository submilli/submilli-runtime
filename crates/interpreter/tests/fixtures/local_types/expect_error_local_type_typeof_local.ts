// expect-error: a type declared in a block can't use `typeof` on the local `x`
// A local type is lifted to module scope, where a local variable doesn't exist.

function main(): void {
  const x = { a: 1 };
  type X = typeof x;
  const y: X = { a: 2 };
  assert(y.a === 2, "unreachable");
}
