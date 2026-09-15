// test262: test/built-ins/String/prototype/split/call-split-h-instance-is-string-hello.js
// The `__split.constructor === Array` reflection check is dropped
// (no constructor property); the element assertions are kept.

function main(): void {
  const split = "hello".split("h");

  assertSameValue(split.length, 2, "The value of split.length is 2");
  assertSameValue(split[0], "", 'The value of split[0] is ""');
  assertSameValue(split[1], "ello", 'The value of split[1] is "ello"');
}
