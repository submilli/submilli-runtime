// Not a test262 port. Documents a deliberate divergence: Set elements compare
// via the structural equals/hash vtable, not SameValueZero reference identity
// as in JS — two objects with equal contents are the same element
// (spec.md §2.7). JS counterparts asserting reference-identity live under
// rejected/Set/.

function main(): void {
  const s = new Set<{ id: number }>();

  s.add({ id: 1 });
  s.add({ id: 1 });
  assertSameValue(s.size, 1, "equal-content objects dedupe to one element");
  assertSameValue(s.has({ id: 1 }), true, "an equal-content literal is found");

  s.add({ id: 2 });
  assertSameValue(s.size, 2, "different contents are a different element");
  assertSameValue(s.delete({ id: 1 }), true, "delete also compares structurally");
  assertSameValue(s.size, 1);
}
