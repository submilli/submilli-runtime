// expect-error-count: 1
// expect-error: `super` is only valid inside a class method or constructor body
// A function expression has its own `this`, so its `super` gets only the
// parser's error, not a second one about reading before `super(...)`.
class Base {
  v: number;
  constructor(x: number) {
    this.v = x;
  }
  twice(): number {
    return this.v * 2;
  }
}

class Derived extends Base {
  constructor() {
    const g = function (): number {
      return super.twice();
    };
    super(1);
  }
}

function main(): void {}
