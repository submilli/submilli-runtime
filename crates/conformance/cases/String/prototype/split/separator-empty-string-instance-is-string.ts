// test262: test/built-ins/String/prototype/split/separator-empty-string-instance-is-string.js
// The `__split.constructor === Array` reflection check is dropped.

function main(): void {
  const split = " ".split("");

  assertSameValue(split.length, 1, "The value of split.length is 1");
  assertSameValue(split[0], " ", 'The value of split[0] is " "');
}
