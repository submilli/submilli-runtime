import { NumBox } from "@test/mid";

function main(): void {
  const n = new NumBox(4);
  assert(n.get() === 4, "inherited implicit ctor across packages");
  assert(n.twice() === 8, "subclass method over an erased inherited slot");
  assert(n.value === 4, "inherited generic field");
}
