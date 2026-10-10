export class Box<T> {
  constructor(public value: T) {}
  applied(f: (x: T) => number): number {
    return f(this.value);
  }
  mapper(): (x: T) => T {
    return (x: T): T => x;
  }
}
