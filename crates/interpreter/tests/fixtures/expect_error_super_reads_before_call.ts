// expect-error-count: 7
// expect-error: the arguments of `super(...)` can't read `this` or a `super` member
// expect-error: the arguments of `super(...)` can't read `this` or a `super` member
// expect-error: `super(...)` must be called before accessing `this` or a `super` member
// expect-error: a `catch` or `finally` around `super(...)` can't read `this` or a `super` member
// expect-error: a `catch` or `finally` around `super(...)` can't read `this` or a `super` member
// expect-error: a `catch` or `finally` around `super(...)` can't read `this` or a `super` member
// expect-error: a `catch` or `finally` around `super(...)` can't read `this` or a `super` member
// Until `super(...)` returns the parent's fields aren't set, so neither its
// arguments, anything before it, nor a `catch` or `finally` around it (which
// also runs when it throws) may read them, even in a function: the parent's
// constructor may call it before the instance is built.
class Base {
  v: number;
  constructor(x: number) {
    this.v = x;
  }
  twice(): number {
    return this.v * 2;
  }
}

class FromThis extends Base {
  w: number = 5;
  constructor() {
    super(this.w + 1);
  }
}

class Callback {
  v: number;
  constructor(read: () => number) {
    this.v = read();
  }
}

class FromCallback extends Callback {
  w: number = 5;
  constructor() {
    super((): number => this.w);
  }
}

class FromSuperMethod extends Base {
  constructor() {
    const n: number = super.twice();
    super(n);
  }
}

class ReadInCatch extends Base {
  w: number = 5;
  constructor(x: number) {
    try {
      super(x);
    } catch (e) {
      console.log(this.w);
      throw e;
    }
  }
}

class ReadInFinally extends Base {
  constructor(x: number) {
    try {
      super(x);
    } finally {
      console.log(super.twice());
    }
  }
}

class ReadAfterNestedTry extends Base {
  w: number = 5;
  constructor(x: number) {
    try {
      try {
        super(x);
      } catch (e) {
        throw e;
      }
    } catch (e) {
      console.log(this.w);
      throw e;
    }
  }
}

class ReadInFinallyAfterCatchCall extends Base {
  w: number = 5;
  constructor(x: number) {
    try {
      throw new Error("first");
    } catch (e) {
      super(x);
    } finally {
      console.log(this.w);
    }
  }
}

function main(): void {
  console.log(new FromThis().v, new FromCallback().v, new FromSuperMethod().v);
  console.log(new ReadInCatch(1).v, new ReadInFinally(1).v);
  console.log(new ReadAfterNestedTry(1).v, new ReadInFinallyAfterCatchCall(1).v);
}
