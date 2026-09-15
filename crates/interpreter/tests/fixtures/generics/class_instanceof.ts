// instanceof on a generic class: bare name tests the erased tag and narrows
// to <unknown, ...> args; negative branch with an unrelated class.
class Box<T> {
  constructor(private value: T) {}
  get(): T {
    return this.value;
  }
}

class Other {
  n: number = 1;
}

function describe(u: unknown): string {
  if (u instanceof Box) {
    // Narrowed to Box<unknown>: methods callable, result is unknown.
    const inner = u.get();
    if (typeof inner === "string") {
      return "box of string " + inner;
    }
    return "box";
  }
  if (u instanceof Other) {
    return "other";
  }
  return "neither";
}

function main(): void {
  const b: unknown = new Box("hi") as unknown;
  assert(describe(b) === "box of string hi", "instanceof + narrowed method call");
  assert(describe(new Other() as unknown) === "other", "unrelated class");
  assert(describe("plain" as unknown) === "neither", "non-class value");

  const u: Box<number> | Other = new Box(3);
  if (u instanceof Box) {
    assert(u.get() === 3, "union operand keeps precise member args");
  } else {
    assert(false, "union narrowing took the wrong branch");
  }
}
