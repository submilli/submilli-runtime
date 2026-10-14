// test262: test/built-ins/Object/assign/target-set-user-error.js
// Adaptation: a class setter replaces defineProperty; the assignment error remains the contract.
class Test262Error extends Error { constructor() { super("setter failed"); } }
class Target { set attr(value: number) { throw new Test262Error(); } }
function main(): void {
  let caught = false;
  try { Object.assign(new Target(), { attr: 1 }); }
  catch (e: Test262Error) { caught = true; }
  assert(caught, "target setter throws Test262Error");
}
