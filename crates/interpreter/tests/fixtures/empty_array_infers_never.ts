// An empty array literal binds its slot's element type to `never`, as tsc
// infers from its `never[]`, and a `never` candidate gives way to the next
// argument's: `a` is `number` below, not the outer call's open `a`.
function merge<a>(as1: a[], as2: a[], cmp: (a1: a, a2: a) => number): a[] {
  return [...as1, ...as2].sort(cmp);
}

function either<T>(a: T, b: T): T {
  return b;
}

function withCallback<T>(a: T, cb: (x: T) => number): number {
  return cb(a);
}
function anyOf<T>(a: T, b: T, c: T): T {
  return c;
}

function main(): void {
  const merged = merge(Array.from([]), [2, 1], (a1, a2) => a1 - a2);
  assert(merged.length === 2, "both elements");
  assert(merged[0] === 1 && merged[1] === 2, "sorted by the callback");
  const none: never[] = [];
  const more: number[] = merge(none, [3], (x, y) => x - y);
  assert(more[0] === 3, "a never candidate gives way");
  const widened = either(Array.from([]), [1]);
  assert(widened.length === 1, "a never[] candidate is no hint for a literal");
  assert(withCallback(none, (x) => x.slice(0).length) === 0, "a callback reads the never[] binding");
  const mixed = anyOf(none, [1], ["a"]);
  assert(mixed.length === 1, "literals after a never[] candidate form their union");
}
