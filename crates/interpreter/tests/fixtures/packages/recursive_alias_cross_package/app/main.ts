import { Node, leaf, total } from "@test/tree";

function main(): void {
  const a: Node = leaf(1);
  const b: Node = { val: 2, kids: [a] };
  assert(total(b) === 3, "walked a recursive alias built across a package boundary");
  assert(b.kids[0].val === 1, "read the imported alias's field one level down");
}
