// expect-error: does not implement `Container<string>`
// A generic class is checked against its interface at opaque parameters, so a
// mismatched instantiation is caught instead of passing on the wildcard rule.
interface Container<T> {
  get(): T;
}

class Box<T> implements Container<string> {
  private value: T;
  constructor(v: T) {
    this.value = v;
  }
  get(): T {
    return this.value;
  }
}

function main(): void {}
