// An interface value interpolates through its `toString`, as `String(x)` does,
// whether the interface declares it as a property, a method, or not at all.
interface Named { toString: () => string; }
interface Shown { toString(): string; }
interface Point { x: number; }
interface Boxed<T> { value: T; toString(): string; }

class Tag {
  toString(): string {
    return "tag";
  }
}

function main(): void {
  const named: Named = { toString: () => "named" };
  const shown: Shown = { toString: () => "shown" };
  const point: Point = { x: 1 };
  const boxed: Boxed<number> = { value: 2, toString: () => "boxed" };
  const tag: Shown = new Tag();
  assert(`${named}` === "named", "a function-typed `toString` property is called");
  assert(`${shown}` === "shown", "a `toString` method is called");
  assert(`${point}` === "[object Object]", "an interface without `toString` uses the default");
  assert(`${boxed}` === String(boxed), "a generic interface matches `String(x)`");
  assert(`<${tag}>` === "<tag>", "a class behind an interface uses its own method");
}
