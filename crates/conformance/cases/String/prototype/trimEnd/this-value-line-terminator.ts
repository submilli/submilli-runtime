// test262: test/built-ins/String/prototype/trimEnd/this-value-line-terminator.js
// The prototype `.call(str)` is a plain method call.

function main(): void {
  // A string of all valid LineTerminator Unicode code points
  const lt = "\u000A\u000D\u2028\u2029";

  const str = lt + "a" + lt + "b" + lt;
  const expected = lt + "a" + lt + "b";

  assertSameValue(str.trimEnd(), expected);
}
