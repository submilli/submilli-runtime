// Field initializers across inheritance: the parent's initializer runs during
// super(...), and the child's own initializer runs right after super returns,
// so it observes post-super state.
class Base {
  kind: string = "base";
  count: number;
  constructor(start: number) {
    this.count = start;
  }
}

class Derived extends Base {
  tag: string = "derived";
  total: number = this.count + 1;
  constructor(start: number) {
    super(start);
  }
}

function main(): void {
  const d = new Derived(7);
  assert(d.kind === "base");
  assert(d.count === 7);
  assert(d.tag === "derived");
  // `this.count` was initialized by the parent ctor before the child initializer ran.
  assert(d.total === 8);
}
