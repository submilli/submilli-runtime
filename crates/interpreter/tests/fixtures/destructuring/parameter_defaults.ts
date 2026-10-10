function pick({ a = 3 }: { a?: number } = {}): number {
  return a;
}

function main(): void {
  assert(pick() === 3, "omitted parameter uses object and binding defaults");
  assert(pick(undefined) === 3, "undefined parameter uses its default");
  assert(pick({ a: 1 }) === 1, "present property keeps its value");
  const read = ([value = 8]: (number | undefined)[] = []) => value;
  assert(read() === 8, "arrow defaults apply to a missing array element");
  assert(read([2]) === 2, "arrow pattern preserves an explicit element");
}
