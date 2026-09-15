// test262: test/built-ins/String/prototype/at/returns-item.js
// The `typeof String.prototype.at` reflection check is dropped (no prototypes).

function main(): void {
  const s = "12345";

  assertSameValue(s.at(0), "1", 's.at(0) must return "1"');
  assertSameValue(s.at(1), "2", 's.at(1) must return "2"');
  assertSameValue(s.at(2), "3", 's.at(2) must return "3"');
  assertSameValue(s.at(3), "4", 's.at(3) must return "4"');
  assertSameValue(s.at(4), "5", 's.at(4) must return "5"');
}
