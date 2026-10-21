// A class that `implements` an object type alias must match its members, as for
// an interface (tsc: TS2416, TS2420).
// expect-error-count: 3
// expect-error: class `Wrong` does not implement `Named`: member `name` has an incompatible signature
// expect-error: class `Incomplete` does not implement `Counter`: missing member `next`
// expect-error: class `Fixed` does not implement `Box<string>`: member `value` has an incompatible signature
type Named = { name: string };
type Counter = { count: number; next(): number };
type Box<T> = { value: T };

class Wrong implements Named {
  name: number = 1;
}
class Incomplete implements Counter {
  count: number = 0;
}
class Fixed<T> implements Box<string> {
  constructor(public value: T) {}
}

function main(): void {}
