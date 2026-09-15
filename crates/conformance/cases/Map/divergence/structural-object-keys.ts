// Not a test262 port. Documents a deliberate divergence: Map keys compare
// via the structural equals/hash vtable, not SameValueZero reference identity
// as in JS — two objects with equal contents are the same key (spec.md §2.7).
// JS counterparts asserting reference-identity keying live under
// rejected/Map/.

function main(): void {
  const m = new Map<{ id: number }, string>();

  m.set({ id: 1 }, "first");
  assertSameValue(m.has({ id: 1 }), true, "an equal-content literal finds the entry");
  assertSameValue(m.get({ id: 1 }), "first");

  m.set({ id: 1 }, "second");
  assertSameValue(m.size, 1, "an equal-content key overwrites, never duplicates");
  assertSameValue(m.get({ id: 1 }), "second");

  m.set({ id: 2 }, "other");
  assertSameValue(m.size, 2, "different contents are a different key");
  assertSameValue(m.delete({ id: 2 }), true, "delete also keys structurally");
}
