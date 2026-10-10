// expect-error-count: 3
// expect-error: `super(...)` is only valid inside a constructor
// expect-error: `super(...)` is only valid inside a constructor
// expect-error: `super(...)` requires a parent class
// Each misplaced `super(...)` gets one error, naming what's wrong with it.
class Base {
  v: number = 1;
}

class Derived extends Base {
  method(): void {
    super();
  }

  static make(): void {
    super();
  }
}

class NoParent {
  constructor() {
    const f = (): void => super();
  }
}

function main(): void {}
