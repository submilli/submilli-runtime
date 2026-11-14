// A generic method in an object literal: called with explicit and inferred
// type arguments, through a field read.
function main(): void {
  const box = {
    id<T>(value: T): T {
      return value;
    },
    pair<A, B>(a: A, b: B): [A, B] {
      return [a, b];
    },
  };
  const n = box.id<number>(1);
  assert(n === 1, "explicit type argument");
  const s = box.id("x");
  assert(s === "x", "inferred type argument");
  const p = box.pair(1, "two");
  assert(p[0] === 1 && p[1] === "two", "two type parameters");
  const arrow = <T,>(value: T): T[] => [value, value];
  assert(arrow(3).length === 2, "generic arrow");
}
