import { Dog, GenHolder, Holder, LibReset, Middle } from "@test/base";

class Narrowed extends Holder {
  v?: Dog;
}

class SameTyped extends Holder {
  tag?: string;
}

class WithInit extends Holder {
  v: Dog | null = new Dog();
}

// Three levels, with both the grandparent and the intermediate imported.
class Leaf extends Middle {
  v?: Dog;
}

class GenLeaf extends GenHolder<string> {
  v?: string;
  constructor() {
    super("p");
  }
}

function main(): void {
  const n = new Narrowed();
  assert(n.v === undefined, "an imported parent's initializer does not leak into the child's slot");
  const asHolder: Holder = n;
  assert(asHolder.v === undefined, "the parent-typed read agrees");
  assert(n.tag === "parent-tag", "a field the child does not redeclare keeps its value");

  const s = new SameTyped();
  assert(s.tag === undefined, "a same-typed redeclaration starts empty");
  assert(s.v !== null && s.v !== undefined, "and leaves the sibling field alone");

  const l = new Leaf();
  assert(l.v === undefined, "a reset over an imported grandparent, through an imported intermediate");
  assert(l.extra === "mid", "the imported intermediate's own field survives");
  assert(l.tag === "parent-tag", "a field the leaf does not redeclare keeps its value");

  const r = new LibReset();
  assert(r.v === undefined, "a reset declared in the library survives layout reconstruction");
  const rAsHolder: Holder = r;
  assert(rAsHolder.v === undefined, "and reads the same through the imported parent type");
  assert(r.tag === "parent-tag", "the library class's other field is untouched");
  assert(JSON.stringify(r) === '{"tag":"parent-tag"}', "and its undefined field is omitted from JSON");

  assert(new GenLeaf().v === undefined, "a reset over a generic imported parent");

  const w = new WithInit().v;
  assert(w !== null, "the child's own initializer survives");
  if (w !== null) {
    assert(w.fetch() === "ball", "and holds the child's value");
  }
}
