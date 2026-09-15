// expect-error: static member `unwrap` cannot reference class type parameter `T`
class Box<T> {
  private v: T;
  constructor(v: T) {
    this.v = v;
  }
  static unwrap(b: Box<T>): T {
    return b.v;
  }
}

function main(): void {}
