// SUB-1755: set algebra takes a set of another element type, as tsc's
// `union<U>(other: ReadonlySetLike<U>): Set<T | U>` family does.
function main(): void {
  const numbers = new Set<number>([1, 2]);
  const strings = new Set<string>(["x"]);
  const both: Set<number | string> = numbers.union(strings);
  assert(both.size === 3);
  assert(numbers.symmetricDifference(strings).size === 3);
  assert(numbers.isDisjointFrom(strings));
  assert(!numbers.isSubsetOf(strings) && !numbers.isSupersetOf(strings));

  const mixed = new Set<number | string>([1, "x"]);
  // The result has the narrower element type of the two.
  const shared: Set<number> = mixed.intersection(numbers);
  const sharedToo: Set<number> = numbers.intersection(mixed);
  assert(shared.size === 1 && sharedToo.size === 1 && shared.has(1));
  const rest: Set<number> = numbers.difference(mixed);
  assert(rest.size === 1 && rest.has(2));
  assert(numbers.isSubsetOf(new Set<number | string>([1, 2, "y"])));
}
