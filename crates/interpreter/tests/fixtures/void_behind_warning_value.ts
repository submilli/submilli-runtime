function nothing(): void {}
function main(): void {
  const n: number = 1;
  const object = { value: n ?? nothing() };
  const array = [n ?? nothing()];
  assert(object.value === 1, "redundant coalescing preserves defined field");
  assert(array[0] === 1, "redundant coalescing preserves defined element");
}
