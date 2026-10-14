// test262: test/built-ins/Object/groupBy/callback-throws.js
// Adaptation: a typed Error subclass replaces Test262Error.
class Test262Error extends Error { constructor(message: string) { super(message); } }
function main(): void {
  let caught = false;
  try { Object.groupBy([1], (): string => { throw new Test262Error("throw in callback"); }); }
  catch (e: Test262Error) { caught = true; }
  assert(caught, "callback throws Test262Error");
}
