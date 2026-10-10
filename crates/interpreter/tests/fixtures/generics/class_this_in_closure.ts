// `this` inside closures in a *generic* class's members: the captured receiver
// and the erased `T` slots have to compose, including when the closure escapes
// the member that created it.
class Box<T> {
  private v: T;
  private tag: string;

  constructor(v: T, tag: string) {
    this.v = v;
    this.tag = ((): string => tag + "!")();
  }

  repeated(): T[] {
    return [1, 2].map((_x: number): T => this.v);
  }

  viaClosure(): T {
    const f = (): T => this.v;
    return f();
  }

  escaping(): () => T {
    return (): T => this.v;
  }

  get label(): string {
    const f = (): string => this.tag;
    return f();
  }
}

function main(): void {
  const s = new Box<string>("q", "t");
  assert(s.repeated()[1] === "q", "this in a closure returning T");
  assert(s.viaClosure() === "q", "T-returning closure over this");
  assert(s.escaping()() === "q", "escaping closure capturing this on a generic class");
  assert(s.label === "t!", "this in an accessor closure");

  const n = new Box<number>(4, "n");
  assert(n.viaClosure() === 4, "same slots at a number instantiation");
  assert(n.escaping()() === 4, "escaping closure at a number instantiation");
}
