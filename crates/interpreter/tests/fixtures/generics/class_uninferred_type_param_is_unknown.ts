// A type parameter nothing infers is `unknown`, as in tsc, so the instance
// takes any value its methods accept as `T`.
class Box<T> {
  private value: T | null;
  constructor() {
    this.value = null;
  }
  put(v: T): void {
    this.value = v;
  }
  isEmpty(): boolean {
    return this.value === null;
  }
}

function main(): void {
  const b = new Box();
  assert(b.isEmpty(), "starts empty");
  b.put(1);
  b.put("one");
  assert(!b.isEmpty(), "holds a value of any type");
}
