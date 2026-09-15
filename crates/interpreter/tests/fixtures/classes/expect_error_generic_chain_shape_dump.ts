// The dump substitutes the parent's type parameters through the chain, and the
// `extends` clause carries the arguments they were substituted against.
// expect-error: field `nope` does not exist on `NumBox`
// expect-error: class NumBox extends Base<number>
// expect-error: v: number;  // from Base
// expect-error: get(): number;  // from Base
class Base<T> {
  v: T;
  constructor(v: T) {
    this.v = v;
  }
  get(): T {
    return this.v;
  }
}

class NumBox extends Base<number> {}

function main(): void {
  const n = new NumBox(1);
  console.log(`${n.nope}`);
}
