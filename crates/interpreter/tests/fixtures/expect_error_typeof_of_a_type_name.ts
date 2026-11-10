// A class, a whole enum or an interface is a type, not a value, so `typeof`
// has nothing to take; neither does a binding inside its own annotation.
// tsc gives `typeof C` a constructor type and `typeof E` an enum-object type,
// which Submilli doesn't have.
// expect-error-count: 4
// expect-error: `Shape` is a class, not a value, so `typeof` has no type to take
// expect-error: `Level` is an enum type, not a value, so `typeof` has no type to take
// expect-error: `Named` is a type, not a value, so `typeof` has no type to take
// expect-error: unresolved identifier `node`
class Shape {
  static unit(): number {
    return 1;
  }
}
enum Level {
  Low,
  High,
}
interface Named {
  name: string;
}

let shapeClass: typeof Shape | null = null;
let levels: typeof Level | null = null;
let named: typeof Named | null = null;
let node: { next: typeof node } | null = null;

function main(): void {}
