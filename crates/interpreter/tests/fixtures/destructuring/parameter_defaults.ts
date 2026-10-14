function pick({ a = 3 }: { a?: number } = {}): number {
  return a;
}

// An unannotated array literal default that a pattern takes apart is a tuple,
// so its elements keep their own types.
function pair([count, label] = [1, "one"]): string {
  return label.repeat(count);
}

function main(): void {
  assert(pick() === 3, "omitted parameter uses object and binding defaults");
  assert(pick(undefined) === 3, "undefined parameter uses its default");
  assert(pick({ a: 1 }) === 1, "present property keeps its value");
  const read = ([value = 8]: (number | undefined)[] = []) => value;
  assert(read() === 8, "arrow defaults apply to a missing array element");
  assert(read([2]) === 2, "arrow pattern preserves an explicit element");
  assert(pair() === "one" && pair([2, "ab"]) === "abab", "a tuple default");
}
