import { Box, Container } from "@t/lib";

class Local<T> implements Container<T> {
  private v: T;
  constructor(v: T) { this.v = v; }
  get(): T { return this.v; }
  put(x: T): void { this.v = x; }
}

function bump(c: Container<number>): number { c.put(c.get() + 1); return c.get(); }

function main(): void {
  assert(bump(new Box(1)) === 2, "imported generic class through an imported generic interface");
  assert(bump(new Local(5)) === 6, "local class implementing an imported generic interface");
}
