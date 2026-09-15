export class Box<T> {
  value: T;
  constructor(v: T) { this.value = v; }
  get(): T { return this.value; }
}
export class Plain { n: number = 1; }
export function makeBox(): Box<number> { return new Box(1); }
export function makePlain(): Plain { return new Plain(); }
