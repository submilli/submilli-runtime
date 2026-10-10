// The defect is not "empty subclass": a subclass that declares its own
// unrelated members still inherits the interface-satisfying ones, and those
// still have to reach the instance payload.
interface Container {
  get(): number;
}

class Base implements Container {
  constructor(private n: number) {}
  get(): number {
    return this.n;
  }
}

class Kid extends Base {
  extra: number;
  constructor(n: number, extra: number) {
    super(n);
    this.extra = extra;
  }
  describe(): string {
    return "kid";
  }
}

function read(c: Container): number {
  return c.get();
}

function main(): void {
  const kid = new Kid(5, 9);
  assert(read(kid) === 5, "inherited method survives alongside new members");
  assert(kid.extra === 9, "subclass's own field");
  assert(kid.describe() === "kid", "subclass's own method");
}
