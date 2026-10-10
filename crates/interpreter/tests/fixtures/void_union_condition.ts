function nothing(): void {}
function main(): void {
  let entered = false;
  if (true ? nothing() : 1) { entered = true; }
  assert(!entered, "void branch in a value union is falsy");
}
