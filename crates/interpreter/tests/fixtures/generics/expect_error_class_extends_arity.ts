// expect-error: class `Box` expects 1 type argument, got 0
// Arity is checked on the `extends` clause, not only in use position.
class Box<T> {
  constructor(public value: T) {}
}

class Kid extends Box {
  constructor() {
    super(1);
  }
}

function main(): void {}
