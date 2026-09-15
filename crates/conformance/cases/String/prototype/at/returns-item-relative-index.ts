// test262: test/built-ins/String/prototype/at/returns-item-relative-index.js

function main(): void {
  const s = "12345";

  assertSameValue(s.at(0), "1", 's.at(0) must return "1"');
  assertSameValue(s.at(-1), "5", 's.at(-1) must return "5"');
  assertSameValue(s.at(-3), "3", 's.at(-3) must return "3"');
  assertSameValue(s.at(-4), "2", 's.at(-4) must return "2"');
}
