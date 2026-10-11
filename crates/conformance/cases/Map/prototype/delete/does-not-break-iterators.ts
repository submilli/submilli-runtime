// test262: test/built-ins/Map/prototype/delete/does-not-break-iterators.js
// Adapted: `n.value[i]` is read through an `as
// IteratorYieldResult<[string, number]>` cast — indexing needs the yield
// variant, and `done` is typed boolean (not a literal), so the IteratorResult
// union is not narrowed by the type alone.

function main(): void {
  const m = new Map<string, number>([
    ["a", 1],
    ["b", 2],
    ["c", 3],
  ]);
  const e = m.entries();

  e.next();
  m.delete("b");

  let n = e.next();

  assertSameValue((n as IteratorYieldResult<[string, number]>).value[0], "c");
  assertSameValue((n as IteratorYieldResult<[string, number]>).value[1], 3);

  n = e.next();
  assertSameValue(n.value, undefined);
  assertSameValue(n.done, true);
}
