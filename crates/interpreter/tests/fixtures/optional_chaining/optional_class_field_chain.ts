class Leaf {
  constructor(public value: string) {}
}

class Node {
  constructor(public leaf: Leaf | null) {}
}

class Root {
  constructor(public node: Node | null) {}
}

function value_of(r: Root | null): string | null {
  return r?.node?.leaf?.value;
}

function main(): void {
  assert(value_of(new Root(new Node(new Leaf("deep")))) === "deep", "three-hop class chain");
  assert(value_of(new Root(new Node(null))) === null, "null at the last hop");
  assert(value_of(new Root(null)) === null, "null at the middle hop");
  assert(value_of(null) === null, "null receiver");
}
