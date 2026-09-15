// test262: test/built-ins/RegExp/prototype/exec/y-init-lastindex.js
// expect-error: cannot assign to readonly property `lastIndex`
// Documented divergence: `lastIndex` is read-only in v1 — the wrapper writes it
// back internally on g/y matches; user-code writes are a planned follow-up
// (RegExp builtin declaration). JS honors a user-set lastIndex for sticky exec.

function main(): void {
  const r = /./y;
  r.lastIndex = 1;
  const m = r.exec("abc");
  assert(m !== null, "sticky exec honors the initial lastIndex");
  if (m === null) {
    return;
  }
  const matched = m.match;
  assertSameValue(matched, "b", "match starts at lastIndex 1");
}
