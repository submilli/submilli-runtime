// expect-error: parameter properties are only allowed in a constructor
function build(public x: number): number {
  return x;
}

function main(): void {
  assert(build(1) === 1);
}
