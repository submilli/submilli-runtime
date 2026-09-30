// Paths that throw needn't call `super(...)`; every other path does, whether
// the call comes first, after a guard, or in a `try`.
class Base {
  v: number;
  constructor(x: number) {
    this.v = x;
  }
}

class Guarded extends Base {
  constructor(x: number) {
    if (x < 0) {
      throw new Error("negative");
    }
    super(x);
  }
}

class EitherBranch extends Base {
  constructor(x: number) {
    const doubled = x * 2;
    if (doubled > 10) {
      throw new Error("too big");
    } else {
      super(doubled);
    }
  }
}

class InTry extends Base {
  constructor(x: number) {
    try {
      super(x);
    } catch (e) {
      throw e;
    }
  }
}

class Parenthesized extends Base {
  constructor(x: number) {
    (super(x));
  }
}

function main(): void {
  assert(new Guarded(3).v === 3, "super after a throwing guard");
  assert(new EitherBranch(4).v === 8, "super in the branch that completes");
  assert(new InTry(5).v === 5, "super in a try whose catch rethrows");
  assert(new Parenthesized(6).v === 6, "a parenthesized super call is a statement");
  let threw = false;
  try {
    new Guarded(-1);
  } catch (e) {
    threw = true;
  }
  assert(threw, "the guard throws");
}
