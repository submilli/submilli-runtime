import { Base, Deeper, Shadowed } from "@test/base";

// Consumer-side shadowing of an imported parent's field: the slot comes from
// the reconstructed prefix, not from a fresh index appended locally.
class LocalShadow extends Base {
  v: string = "local-v";
}

class LocalOverShadowed extends Shadowed {
  v: string = "local-over-v";
}

function main(): void {
  const s = new Shadowed();
  const asBase: Base = s;
  assert(s.a === "base-a", "non-shadowed inherited field across the boundary");
  assert(s.v === "shadowed-v", "producer-side shadowing reconstructs to one slot");
  assert(s.z === "shadowed-z", "the producer subclass's own new field");
  assert(asBase.v === "shadowed-v", "imported-parent-typed read of a shadowed field");
  assert(s.read() === "shadowed-v", "inherited method reads the shared slot");

  const d = new Deeper();
  assert(d.v === "deeper-v", "two shadowing links in the imported chain");
  assert(d.z === "shadowed-z", "a field introduced between the shadowing links");
  assert(d.read() === "deeper-v", "inherited method across two shadowing links");

  const l = new LocalShadow();
  const lAsBase: Base = l;
  assert(l.a === "base-a", "local subclass inherits the imported prefix");
  assert(l.v === "local-v", "local shadowing of an imported parent's field");
  assert(lAsBase.v === "local-v", "imported-parent-typed read of a local shadow");
  assert(l.read() === "local-v", "imported method reads the locally shadowed slot");

  const o = new LocalOverShadowed();
  const oAsShadowed: Shadowed = o;
  assert(o.v === "local-over-v", "local shadow of an already-shadowed imported field");
  assert(o.z === "shadowed-z", "the imported subclass's own field stays put");
  assert(oAsShadowed.v === "local-over-v", "read through the imported subclass type");
  assert(o.read() === "local-over-v", "imported root method reads the local shadow");

  assert(
    JSON.stringify(s) === '{"a":"base-a","v":"shadowed-v","z":"shadowed-z"}',
    "the shadowed field serializes once across the boundary",
  );
}
