function main(): void {
  let nested: unknown[] = [42];
  for (let i = 0; i < 130; i++) {
    nested = [nested];
  }
  let limited = false;
  try {
    nested.flat(200);
  } catch (e: RangeError) {
    limited = e.message.includes("levels of nesting");
  }
  assert(limited, "flat limits its explicit traversal stack");
  assert(nested.flat(2).length === 1, "requested shallow traversal still works");
  assert(Object.is([[1, 2], [3]].flat(), [1, 2, 3]), "sibling order is preserved");
}
