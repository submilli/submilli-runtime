function nothing(): void {}
function main(): void {
  const object: { value: void } = { value: nothing() };
  assert(object.value === undefined, "object annotation accepts void");
}
