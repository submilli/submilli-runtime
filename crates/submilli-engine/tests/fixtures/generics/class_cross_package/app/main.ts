import { Box } from "@test/boxes";

class NumBox extends Box<number> {
  doubled(): number {
    return this.get() * 2;
  }
}

function main(): void {
  const n = new Box(21);
  n.set(n.get() + 21);
  assert(n.get() === 42, "imported generic class, number instantiation");

  const s = new Box<string>("cross");
  s.value = s.value + "-package";
  assert(s.value === "cross-package", "T field write on an imported class");

  const nb = new NumBox(10);
  assert(nb.doubled() === 20, "local subclass of imported generic parent");
  assert(nb.value === 10, "inherited field across the package boundary");
  const asBox: Box<number> = nb;
  assert(asBox.get() === 10, "parent-typed dispatch across the boundary");
}
