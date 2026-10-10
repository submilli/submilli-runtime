// A get-only (read-only) accessor cannot satisfy a writable interface property:
// a write through the interface reference would have nowhere to land. Marking
// the interface property `readonly` is the fix.
// expect-error: does not implement
interface HasCount {
  count: number;
}

class Counter implements HasCount {
  get count(): number {
    return 1;
  }
}

export function main(): string {
  const c: Counter = new Counter();
  return c.count.toString();
}
