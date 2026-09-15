export interface Container<T> { get(): T; put(v: T): void; }
export class Box<T> implements Container<T> {
  private v: T;
  constructor(v: T) { this.v = v; }
  get(): T { return this.v; }
  put(x: T): void { this.v = x; }
}
