// expect-error-count: 2
// expect-error: `super(...)` can't be called from a function nested in a constructor
// expect-error: a subclass constructor must call `super(...)`
// A `super(...)` inside an arrow may run late or never, so it isn't the
// constructor's call (SUB-1159).
class Base {
  v: number;
  constructor(x: number) {
    this.v = x;
  }
}

class Derived extends Base {
  constructor() {
    const r = (): void => super(1);
  }
}

function main(): void {
  console.log(new Derived().v + 1);
}
