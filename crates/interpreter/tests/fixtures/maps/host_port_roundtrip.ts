// End-to-end backstop for the Rust-host Map: object keys compared structurally
// through the vtable, construction driven from a host-built iterator, and the
// host-built IteratorResult walked by hand via next()/done.

function main(): void {
  // Object keys: structural equality on get/set/has/delete.
  const m = new Map<{ id: number }, string>();
  const a = { id: 1 };
  const b = { id: 2 };
  m.set(a, "a");
  m.set(b, "b");
  assert(m.size === 2, "two object keys");
  assert(m.has({ id: 1 }), "structural key lookup");
  const got: string | null = m.get({ id: 2 });
  assert(got === "b", "structural get");
  assert(m.delete({ id: 1 }), "delete by structural key");
  assert(!m.has(a), "deleted key is gone");
  assert(m.size === 1, "size reflects the delete");

  // Construct from an iterator (host-driven), preserving entries.
  const src = new Map<string, number>();
  src.set("x", 1);
  src.set("y", 2);
  src.set("z", 3);
  const copy = new Map(src.entries());
  assert(copy.size === 3, "constructed from an iterator");
  const y: number | null = copy.get("y");
  assert(y === 2, "copied value survives the round-trip");

  // Drive the host-built keys() cursor by hand.
  let count: number = 0;
  const it = copy.keys();
  while (true) {
    const r = it.next();
    if (r.done) {
      break;
    }
    count = count + 1;
  }
  assert(count === 3, "manual next() over keys() yields each entry once");
}
