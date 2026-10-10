import { Circle, Sized } from "@test/geo";

function readSize(s: Sized): number {
  return s.size;
}

function main(): void {
  const c = new Circle(5);
  // Class-typed accessor read/write on an imported class (B2 over imported slots).
  assert(c.size === 10);
  c.size = 20;
  assert(c.size === 20);

  // Through the interface the imported class implements (dynamic dispatch).
  const s: Sized = c;
  assert(readSize(s) === 20);
  s.size = 8;
  assert(readSize(s) === 8);
}
