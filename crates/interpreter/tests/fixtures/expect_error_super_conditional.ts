// expect-error-count: 4
// expect-error: this constructor can finish without calling `super(...)`
// expect-error: this `return` can run before `super(...)`
// expect-error: `super(...)` must be a statement of its own
// A `super(...)` some path skips would leave the parent's fields unset
// (SUB-1159).
class Base {
  v: number;
  constructor(x: number) {
    this.v = x;
  }
}

class Conditional extends Base {
  constructor(c: boolean) {
    if (c) {
      super(1);
    }
  }
}

class EarlyReturn extends Base {
  constructor(c: boolean) {
    if (c) {
      return;
    }
    super(1);
  }
}

class AfterReturn extends Base {
  constructor() {
    return;
    super(1);
  }
}

class InExpression extends Base {
  constructor(c: boolean) {
    c ? super(1) : console.log("skipped");
  }
}

function main(): void {
  console.log(new Conditional(false).v);
  console.log(new EarlyReturn(true).v);
  console.log(new InExpression(false).v);
  console.log(new AfterReturn().v);
}
