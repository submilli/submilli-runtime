function nothing(): void {}
function main(): void {
  const values = [nothing()];
  const object = { value: nothing() };
  assert(values[0] === undefined, "inferred void array element");
  assert(object.value === undefined, "inferred void object field");
}
