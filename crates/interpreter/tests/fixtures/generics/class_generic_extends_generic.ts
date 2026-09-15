// A generic child extending a generic parent: extends args carry the child's
// own type param through the chain-walk substitution.
class Box<T> {
  value: T;
  constructor(v: T) {
    this.value = v;
  }
  get(): T {
    return this.value;
  }
}

class Tagged<T> extends Box<T> {
  readonly tag: string;
  constructor(v: T, tag: string) {
    super(v);
    this.tag = tag;
  }
  labelled(): string {
    return this.tag + "!";
  }
}

function main(): void {
  const t = new Tagged(7, "seven");
  assert(t.get() === 7, "inherited generic method at the child's T");
  assert(t.tag === "seven", "child's own field");
  assert(t.labelled() === "seven!", "child method using its own members");

  const s = new Tagged<string>("x", "letter");
  assert(s.value === "x", "inherited generic field");

  const asBox: Box<number> = t;
  assert(asBox.get() === 7, "assignable to parent at matching args");
}
