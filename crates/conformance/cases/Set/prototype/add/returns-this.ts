// test262: test/built-ins/Set/prototype/add/returns-this.js
// Adapted: SameValue identity over objects does not port (structural ===);
// chainability is asserted by mutating the returned set and observing `s`.

function main(): void {
  const s = new Set<number>();

  const returned = s.add(1);
  returned.add(2);

  assertSameValue(s.size, 2, "`s.add(1)` returns `s`");
  assertSameValue(s.has(2), true, "mutating the returned set mutates `s`");
}
