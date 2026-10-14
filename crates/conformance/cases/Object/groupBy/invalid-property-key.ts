// test262: test/built-ins/Object/groupBy/invalid-property-key.js
// Adaptation: a class replaces the object with a toString method.
class Test262Error extends Error { constructor(message: string) { super(message); } }
class InvalidKey {
  toString(): string { throw new Test262Error("not a property key"); }
}
function main(): void {
  let caught = false;
  try { Object.groupBy([1], (): InvalidKey => new InvalidKey()); }
  catch (e: Test262Error) { caught = true; }
  assert(caught, "key conversion throws Test262Error");
}
