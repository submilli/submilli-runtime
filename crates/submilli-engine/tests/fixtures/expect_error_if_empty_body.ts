// expect-error: `if` has an empty body
// `if (c);` detaches the block meant to follow it, so unlike a loop it has no empty body.
function main(): void {
  const c = true;
  if (c);
}
