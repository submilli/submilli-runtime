function nothing(): void {}
function main(): void {
  assert(nothing() === nothing(), "void calls compare equal");
  assert(nothing() === undefined, "void call compares equal to undefined");
}
