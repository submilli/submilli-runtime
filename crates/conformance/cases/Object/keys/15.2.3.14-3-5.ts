// test262: test/built-ins/Object/keys/15.2.3.14-3-5.js
// Object.keys must return a fresh array on each invocation.

function main(): void {
  const literal = {
    a: 1,
  };
  const keysBefore: string[] = Object.keys(literal);
  assertSameValue(keysBefore[0], "a", "keysBefore[0]");
  keysBefore[0] = "x";
  const keysAfter: string[] = Object.keys(literal);

  assertSameValue(keysBefore[0], "x", "keysBefore[0]");
  assertSameValue(keysAfter[0], "a", "keysAfter[0]");
}
