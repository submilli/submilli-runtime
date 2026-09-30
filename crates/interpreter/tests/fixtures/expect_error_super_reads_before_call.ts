// expect-error-count: 3
// expect-error: the arguments of `super(...)` can't read `this` or a `super` member
// expect-error: the arguments of `super(...)` can't read `this` or a `super` member
// expect-error: `super(...)` must be called before accessing `this` or a `super` member
// Until `super(...)` returns the parent's fields aren't set, so neither its
// arguments nor anything before it may read them, even in a function: the
// parent's constructor may call it before the fields are set.
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

function main(): void {
  console.log(new FromThis().v, new FromCallback().v, new FromSuperMethod().v);
}
