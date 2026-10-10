// expect-error: static member `make` cannot reference class type parameter `T`
class Box<T> {
  constructor(public value: T) {}
  static make(v: T): Box<T> {
    return new Box(v);
  }
}

function main(): void {}
