// A function expression with no `this` annotation and no object literal to
// bind it has no receiver, so `this` is rejected as it is outside a class
// (tsc: `this` implicitly has type `any`). A function-valued object field still
// binds `this` to the object.
// expect-error-count: 1
// expect-error: `this` is only valid inside a class method or constructor body
function main(): void {
  const unbound = function (): void {
    console.log(this);
  };
  unbound();
  const counter = {
    n: 1,
    read: function (): number {
      return this.n;
    },
  };
  console.log(counter.read());
}
