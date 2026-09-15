// test262: test/built-ins/String/prototype/split/call-split-2-instance-is-string-one-two-three-four-five.js
// The `__split.constructor === Array` reflection check is dropped.

function main(): void {
  const split = "one two three four five".split(/ /, 2);

  assertSameValue(split.length, 2, "The value of split.length is 2");
  assertSameValue(split[0], "one", 'The value of split[0] is "one"');
  assertSameValue(split[1], "two", 'The value of split[1] is "two"');
}
