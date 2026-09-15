// test262: test/built-ins/Object/is/same-value-x-y-string.js
// The `String('foo')` wrapper variant is replaced with a computed string —
// the intent (same code-unit sequence from distinct productions) is kept.

function main(): void {
  assertSameValue(Object.is("", ""), true, "`Object.is('', '')` returns `true`");
  assertSameValue(Object.is("foo", "foo"), true, "`Object.is('foo', 'foo')` returns `true`");
  const computed: string = "f" + "oo";
  assertSameValue(Object.is(computed, "foo"), true, "computed string matches literal");
}
