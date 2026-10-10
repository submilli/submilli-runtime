export class Box<T> {
  value: T;
  constructor(v: T) { this.value = v; }
  get(): T { return this.value; }
  pair(a: T, b: T): T { return a; }
}
